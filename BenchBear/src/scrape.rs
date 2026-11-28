use anyhow::{anyhow, Result};
use regex::Regex;
use reqwest::header::{ACCEPT, ACCEPT_LANGUAGE};
use reqwest::redirect::Policy;
use serde_json::Value;
use std::net::IpAddr;
use std::time::Duration;

use crate::db::{round2, Database};
use crate::models::{CandidateImage, ProductUrlDraft};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

const MAX_HTML_BYTES: usize = 4 * 1024 * 1024;

pub fn validate_public_http_url(raw_url: &str) -> Result<(String, String)> {
    let trimmed = raw_url.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("URL cannot be empty"));
    }
    let parsed = reqwest::Url::parse(trimmed)
        .map_err(|e| anyhow!("Invalid URL: {}", e))?;
    match parsed.scheme() {
        "http" | "https" => {}
        other => return Err(anyhow!("Unsupported URL scheme '{}'; use http:// or https://", other)),
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow!("URL must include a hostname"))?
        .to_ascii_lowercase();

    if host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host.ends_with(".internal")
    {
        return Err(anyhow!("Local hostnames are not allowed: {}", host));
    }

    let clean_host = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = clean_host.parse::<IpAddr>() {
        if is_private_or_loopback_ip(ip) {
            return Err(anyhow!("Private or loopback IP addresses are not allowed: {}", ip));
        }
    }

    Ok((parsed.to_string(), host))
}

fn is_private_or_loopback_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.octets()[0] == 0
                || (v4.octets()[0] == 100 && (v4.octets()[1] & 0b1100_0000) == 64)
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                || (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

pub async fn fetch_product_url(raw_url: &str, db: &Database) -> Result<ProductUrlDraft> {
    let (clean_url, host) = validate_public_http_url(raw_url)?;
    let mut draft = base_draft_from_url(&clean_url, &host);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(Policy::limited(5))
        .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36")
        .build()?;

    let resp_res = client
        .get(&clean_url)
        .header(ACCEPT, "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
        .header(ACCEPT_LANGUAGE, "en-US,en;q=0.9")
        .send()
        .await;

    match resp_res {
        Ok(resp) => {
            let status = resp.status();
            let final_url = resp.url().to_string();
            let final_host = resp
                .url()
                .host_str()
                .map(|s| s.to_ascii_lowercase())
                .unwrap_or_else(|| host.clone());

            if final_url != clean_url {
                draft.url = final_url.clone();
                let re_base = base_draft_from_url(&final_url, &final_host);
                if draft.supplier_sku.is_empty() {
                    draft.supplier_sku = re_base.supplier_sku;
                }
                if draft.name.is_empty() {
                    draft.name = re_base.name;
                }
                if draft.supplier_name.is_empty() {
                    draft.supplier_name = re_base.supplier_name;
                }
            }

            let bytes = resp.bytes().await.unwrap_or_default();
            let slice = if bytes.len() > MAX_HTML_BYTES {
                &bytes[..MAX_HTML_BYTES]
            } else {
                &bytes[..]
            };
            let html = String::from_utf8_lossy(slice).into_owned();

            if is_antibot_or_captcha(status.as_u16(), &html) {
                draft.blocked = true;
                draft.warning = format!(
                    "{} returned an anti-bot or CAPTCHA page (HTTP {}); filled available fields from the URL so you can review and finish manually.",
                    if draft.supplier_name.is_empty() { "The store" } else { &draft.supplier_name },
                    status.as_u16()
                );
            } else if !status.is_success() {
                draft.warning = format!(
                    "Store returned HTTP {}; filled available fields from the URL.",
                    status.as_u16()
                );
            } else {
                enrich_draft_from_html(&mut draft, &html);
                if draft.name.trim().is_empty() && draft.images.is_empty() {
                    draft.blocked = true;
                    draft.warning = format!(
                        "{} returned a page without product metadata (possible bot check or JS-rendered page); filled available URL/SKU fields so you can enter the Part Name manually.",
                        if draft.supplier_name.is_empty() { "The store" } else { &draft.supplier_name }
                    );
                } else {
                    draft.fetched = true;
                }
            }
        }
        Err(e) => {
            draft.warning = format!(
                "Could not fetch page ({}); filled available fields from the URL.",
                e
            );
        }
    }

    finalize_draft(&mut draft, db)?;
    Ok(draft)
}

pub fn base_draft_from_url(url: &str, host: &str) -> ProductUrlDraft {
    let supplier_name = detect_supplier_name(host);
    let mut supplier_sku = String::new();
    let mut fallback_name = String::new();

    if supplier_name == "Amazon" {
        if let Some(asin) = capture_first(url, r"(?i)/(?:dp|gp/product)/([A-Z0-9]{10})(?:[/?#]|$)") {
            supplier_sku = asin.to_uppercase();
        }
        if let Some(slug) = capture_first(url, r"(?i)amazon\.[^/]+/([^/]+)/(?:dp|gp/product)/") {
            if !slug.eq_ignore_ascii_case("dp") && !slug.eq_ignore_ascii_case("gp") {
                fallback_name = slug.replace(['-', '+'], " ").trim().to_string();
            }
        }
    } else if supplier_name == "LCSC" {
        if let Some(code) = capture_first(url, r"(?i)\b(C\d{4,})\b") {
            supplier_sku = code.to_uppercase();
        }
    } else if supplier_name == "Adafruit" {
        if let Some(pid) = capture_first(url, r"(?i)/product/(\d+)") {
            supplier_sku = pid;
        }
    } else if supplier_name == "SparkFun" {
        if let Some(pid) = capture_first(url, r"(?i)/products/(\d+)") {
            supplier_sku = pid;
        }
    } else if supplier_name == "AliExpress" {
        if let Some(pid) = capture_first(url, r"(?i)/item/(\d+)") {
            supplier_sku = pid;
        }
    }

    ProductUrlDraft {
        url: url.to_string(),
        fetched: false,
        blocked: false,
        warning: String::new(),
        suggested_sku: String::new(),
        category: "hardware".to_string(),
        name: fallback_name,
        spec: String::new(),
        description: String::new(),
        package: String::new(),
        unit_cost: 0.0,
        pack_qty: 1,
        total_price: 0.0,
        currency: "USD".to_string(),
        supplier_name,
        supplier_sku,
        mpn: String::new(),
        datasheet_url: String::new(),
        images: Vec::new(),
    }
}

pub fn is_antibot_or_captcha(status: u16, html: &str) -> bool {
    let lower = html.to_ascii_lowercase();
    if lower.contains("validatecaptcha")
        || lower.contains("opfcaptcha")
        || lower.contains("type the characters you see in this image")
        || lower.contains("click the button below to continue shopping")
        || lower.contains("sorry, we just need to make sure you're not a robot")
        || lower.contains("api-services-support@amazon.com")
        || lower.contains("cf-turnstile")
        || lower.contains("challenge-platform")
        || lower.contains("enable javascript and cookies to continue")
        || (lower.contains("just a moment...") && html.len() < 32_000)
        || (lower.contains("a-no-js")
            && html.len() < 16_000
            && !lower.contains("producttitle")
            && !lower.contains("application/ld+json"))
    {
        return true;
    }
    matches!(status, 403 | 429 | 503) && !lower.contains("application/ld+json") && !lower.contains("producttitle")
}

pub fn enrich_draft_from_html(draft: &mut ProductUrlDraft, html: &str) {
    let mut title = String::new();
    let mut desc = String::new();
    let mut price: Option<f64> = None;
    let mut currency = String::new();
    let mut sku_val = String::new();
    let mut mpn_val = String::new();

    // 1. JSON-LD <script type="application/ld+json">
    if let Ok(re_ld) = Regex::new(r#"(?is)<script[^>]*type\s*=\s*["']application/ld\+json["'][^>]*>(.*?)</script>"#) {
        for cap in re_ld.captures_iter(html) {
            if let Ok(json_val) = serde_json::from_str::<Value>(cap[1].trim()) {
                extract_product_jsonld(&json_val, &mut title, &mut desc, &mut price, &mut currency, &mut sku_val, &mut mpn_val);
            }
        }
    }

    // 2. OpenGraph / Meta tags
    if title.is_empty() {
        title = extract_meta_content(html, "og:title")
            .or_else(|| extract_meta_content(html, "twitter:title"))
            .unwrap_or_default();
    }
    if desc.is_empty() {
        desc = extract_meta_content(html, "og:description")
            .or_else(|| extract_meta_content(html, "description"))
            .unwrap_or_default();
    }
    if price.is_none() {
        if let Some(p) = extract_meta_content(html, "product:price:amount")
            .or_else(|| extract_meta_content(html, "og:price:amount"))
        {
            price = parse_price_str(&p);
        }
    }
    if currency.is_empty() {
        if let Some(c) = extract_meta_content(html, "product:price:currency")
            .or_else(|| extract_meta_content(html, "og:price:currency"))
        {
            currency = c;
        }
    }

    // 3. Site-specific / HTML fallbacks (Amazon productTitle & prices, <title>)
    if draft.supplier_name == "Amazon" || title.is_empty() {
        if let Some(amz_title) = capture_first(html, r#"(?is)<span[^>]*id\s*=\s*["']productTitle["'][^>]*>(.*?)</span>"#) {
            let clean = strip_tags(&amz_title);
            if !clean.is_empty() {
                title = clean;
            }
        }
    }
    if title.is_empty() {
        if let Some(raw_t) = capture_first(html, r"(?is)<title[^>]*>(.*?)</title>") {
            title = strip_tags(&raw_t);
        }
    }

    if price.is_none() {
        if let Some(offscreen) = capture_first(html, r#"(?is)class\s*=\s*["'][^"']*a-offscreen[^"']*["'][^>]*>\s*([$€£]?\s*[0-9,]+\.\d{2})"#) {
            price = parse_price_str(&offscreen);
        }
    }
    if price.is_none() {
        if let (Some(w), Some(f)) = (
            capture_first(html, r#"(?is)class\s*=\s*["'][^"']*a-price-whole[^"']*["'][^>]*>\s*([0-9,]+)"#),
            capture_first(html, r#"(?is)class\s*=\s*["'][^"']*a-price-fraction[^"']*["'][^>]*>\s*(\d{2})"#),
        ) {
            price = parse_price_str(&format!("{}.{}", w, f));
        }
    }

    // Datasheet PDF link heuristic
    if draft.datasheet_url.is_empty() {
        if let Some(pdf_url) = capture_first(
            html,
            r#"(?is)href\s*=\s*["'](https?://[^"']+\.pdf(?:\?[^"']*)?)["'][^>]*(?:datasheet|spec|manual|doc)"#,
        )
        .or_else(|| {
            capture_first(
                html,
                r#"(?is)(?:datasheet|technical\s+details)[^<]{0,120}href\s*=\s*["'](https?://[^"']+\.pdf(?:\?[^"']*)?)["']"#,
            )
        }) {
            draft.datasheet_url = decode_html_entities(&pdf_url);
        }
    }

    let cleaned_title = clean_store_title(&decode_html_entities(&title));
    if !cleaned_title.is_empty() {
        if let Some(pack) = detect_pack_qty(&cleaned_title) {
            draft.pack_qty = pack;
        }
    }

    let cleaned_desc = clean_store_title(&decode_html_entities(&strip_tags(&desc)));
    if !cleaned_title.is_empty() {
        let (concise_name, detailed_spec) = split_concise_name_and_spec(&cleaned_title, &cleaned_desc);
        draft.name = concise_name;
        draft.spec = detailed_spec;
    } else if !cleaned_desc.is_empty() {
        draft.spec = truncate_clean(&cleaned_desc, 160);
    }
    draft.description = extract_product_description(html, &desc);

    if draft.supplier_sku.is_empty() && !sku_val.is_empty() {
        draft.supplier_sku = sku_val;
    }
    if draft.mpn.is_empty() && !mpn_val.is_empty() {
        draft.mpn = mpn_val;
    }
    if !currency.is_empty() {
        draft.currency = currency.trim().to_uppercase();
    }

    if let Some(total) = price {
        if total > 0.0 {
            draft.total_price = round2(total);
            draft.unit_cost = round2(total);
        }
    }

    draft.images = extract_gallery_images(html, &draft.url);
}

pub fn finalize_draft(draft: &mut ProductUrlDraft, db: &Database) -> Result<()> {
    let combined = format!("{} {} {}", draft.name, draft.spec, draft.description);
    draft.category = classify_part_category(&combined);

    if draft.package.is_empty() {
        draft.package = detect_package(&combined);
    }

    if draft.pack_qty <= 1 {
        if let Some(pack) = detect_pack_qty(&combined) {
            draft.pack_qty = pack;
        }
    }
    if draft.total_price > 0.0 && draft.pack_qty > 1 {
        draft.unit_cost = round2(draft.total_price / draft.pack_qty as f64);
        if draft.warning.is_empty() {
            draft.warning = format!(
                "Detected pack of {} (${:.2} total -> ${:.2}/ea).",
                draft.pack_qty, draft.total_price, draft.unit_cost
            );
        }
    }

    draft.url = canonicalize_product_url(&draft.url, &draft.supplier_name, &draft.supplier_sku);

    if draft.suggested_sku.is_empty() {
        draft.suggested_sku = db.next_sku_for_category(&draft.category)?;
    }

    Ok(())
}

pub fn canonicalize_product_url(raw_url: &str, supplier: &str, supplier_sku: &str) -> String {
    let Ok(mut parsed) = reqwest::Url::parse(raw_url) else {
        return raw_url.to_string();
    };
    let host = parsed.host_str().unwrap_or("").to_ascii_lowercase();

    if supplier == "Amazon" && supplier_sku.len() == 10 && supplier_sku.chars().all(|c| c.is_ascii_alphanumeric()) {
        return format!("{}://{}/dp/{}", parsed.scheme(), host, supplier_sku.to_uppercase());
    }
    if supplier == "Adafruit" && !supplier_sku.is_empty() && supplier_sku.chars().all(|c| c.is_ascii_digit()) {
        return format!("{}://{}/product/{}", parsed.scheme(), host, supplier_sku);
    }
    if supplier == "SparkFun" && !supplier_sku.is_empty() && supplier_sku.chars().all(|c| c.is_ascii_digit()) {
        return format!("{}://{}/products/{}", parsed.scheme(), host, supplier_sku);
    }
    if supplier == "AliExpress" && !supplier_sku.is_empty() && supplier_sku.chars().all(|c| c.is_ascii_digit()) {
        return format!("{}://{}/item/{}.html", parsed.scheme(), host, supplier_sku);
    }

    // Strip common tracking query parameters on all other stores
    let kept: Vec<(String, String)> = parsed
        .query_pairs()
        .filter(|(k, _)| {
            let kl = k.to_ascii_lowercase();
            !(kl.starts_with("utm_")
                || matches!(
                    kl.as_str(),
                    "ref" | "ref_" | "qid" | "sr" | "sprefix" | "keywords" | "dib" | "dib_tag" | "fbclid" | "gclid" | "psc" | "th"
                ))
        })
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();

    if kept.is_empty() {
        parsed.set_query(None);
    } else {
        let mut qp = parsed.query_pairs_mut();
        qp.clear();
        for (k, v) in kept {
            qp.append_pair(&k, &v);
        }
    }
    parsed.to_string()
}

pub fn split_concise_name_and_spec(raw_title: &str, existing_desc: &str) -> (String, String) {
    // 1. Strip leading pack count prefixes ("3PCS ", "5-Pack ", "Pack of 10 ")
    let mut cleaned = raw_title.trim().to_string();
    for pat in [
        r"(?i)^(?:\(\s*\d{1,4}\s*(?:pcs|pieces|pack)\s*\)|\d{1,4}\s*[- ]?(?:pcs|pieces|pack)|pack\s+of\s+\d{1,4})\s*[-,:]*\s*",
    ] {
        if let Ok(re) = Regex::new(pat) {
            cleaned = re.replace(&cleaned, "").trim().to_string();
        }
    }
    // Strip trailing "(5pcs)" / "5-Pack" too
    if let Ok(re_trail) = Regex::new(r"(?i)\s*(?:\(\s*\d{1,4}\s*(?:pcs|pieces|pack)\s*\)|,\s*\d{1,4}\s*[- ]?(?:pcs|pieces|pack))$") {
        cleaned = re_trail.replace(&cleaned, "").trim().to_string();
    }

    // 2. Deduplicate adjacent repeated brand/chip prefixes (e.g. "ESP32 ESP32-S3" -> "ESP32-S3")
    let tokens: Vec<&str> = cleaned.split_whitespace().collect();
    let mut deduped: Vec<&str> = Vec::with_capacity(tokens.len());
    for (i, tok) in tokens.iter().enumerate() {
        if let Some(&next) = tokens.get(i + 1) {
            let tl = tok.to_ascii_lowercase();
            let nl = next.to_ascii_lowercase();
            if tl == nl || (tl.len() >= 3 && nl.starts_with(&(tl.clone() + "-"))) {
                continue;
            }
        }
        deduped.push(tok);
    }
    let normalized = deduped.join(" ");

    // 3. Extract concise name at first SEO delimiter or word boundary <= 56 chars
    let mut name_part = normalized.clone();
    for delim in [", ", " | ", " - ", " — ", " with ", " With ", " for ", " For ", " compatible ", " Compatible ", " suitable ", " ideal "] {
        if let Some(pos) = name_part.find(delim) {
            if pos >= 12 {
                name_part = name_part[..pos].trim().to_string();
            }
        }
    }
    let concise_name = truncate_word_boundary(&name_part, 56);

    // 4. Preserve remaining technical specs in `spec`
    let remainder = normalized
        .strip_prefix(&concise_name)
        .unwrap_or(&normalized)
        .trim_start_matches(|c: char| matches!(c, ',' | '-' | '|' | ':' | ' '))
        .trim()
        .to_string();

    let spec = if !remainder.is_empty() && remainder.len() >= 8 {
        truncate_clean(&remainder, 160)
    } else if !existing_desc.is_empty() && !existing_desc.eq_ignore_ascii_case(&concise_name) {
        truncate_clean(existing_desc, 160)
    } else {
        truncate_clean(&normalized, 160)
    };

    (concise_name, spec)
}

fn extract_product_description(html: &str, fallback_desc: &str) -> String {
    let mut bullets: Vec<String> = Vec::new();
    let mut seen_lower: Vec<String> = Vec::new();

    if let Some(ul_body) = capture_first(
        html,
        r#"(?is)id\s*=\s*["']feature-bullets["'][^>]*>.*?<ul[^>]*>(.*?)</ul>"#,
    ) {
        let item_re = Regex::new(
            r#"(?is)<span[^>]*class\s*=\s*["'][^"']*a-list-item[^"']*["'][^>]*>(.*?)</span>"#,
        )
        .ok()
        .filter(|re| re.is_match(&ul_body))
        .or_else(|| Regex::new(r#"(?is)<li[^>]*>(.*?)</li>"#).ok());

        if let Some(re) = item_re {
            for cap in re.captures_iter(&ul_body) {
                let text = decode_html_entities(&strip_tags(&cap[1]))
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                let lower = text.to_ascii_lowercase();
                if text.chars().count() < 4
                    || lower.contains("make sure this fits")
                    || lower.contains("see more product details")
                    || lower.contains("report an issue")
                    || seen_lower.iter().any(|s| s == &lower)
                {
                    continue;
                }
                seen_lower.push(lower);
                bullets.push(format!("- {}", text));
            }
        }
    }

    let prod_desc = capture_first(
        html,
        r#"(?is)<div[^>]*id\s*=\s*["']productDescription["'][^>]*>(.*?)</div>"#,
    )
    .map(|raw| {
        decode_html_entities(&strip_tags(&raw))
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    })
    .unwrap_or_default();

    let mut out = if !bullets.is_empty() {
        bullets.join("\n")
    } else {
        String::new()
    };

    if !prod_desc.is_empty() && prod_desc.len() >= 15 {
        if out.is_empty() {
            out = prod_desc;
        } else if out.chars().count() < 250
            && !out.to_ascii_lowercase().contains(&prod_desc.to_ascii_lowercase())
        {
            out = format!("{}\n\n{}", out, prod_desc);
        }
    }

    if out.is_empty() {
        let cleaned = clean_store_title(&decode_html_entities(&strip_tags(fallback_desc)))
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if !cleaned.is_empty() {
            out = cleaned;
        }
    }

    if out.is_empty() {
        String::new()
    } else {
        truncate_word_boundary(&out, 1500)
    }
}

fn truncate_word_boundary(s: &str, max_chars: usize) -> String {
    let trimmed = s.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.trim_end_matches(|c: char| matches!(c, ',' | '-' | '|' | ':')).trim().to_string();
    }
    let mut last_space_byte = None;
    let mut cutoff_byte = trimmed.len();
    for (idx, (bpos, ch)) in trimmed.char_indices().enumerate() {
        if ch.is_whitespace() && idx <= max_chars {
            last_space_byte = Some(bpos);
        }
        if idx == max_chars {
            cutoff_byte = bpos;
            break;
        }
    }
    let slice_end = last_space_byte.filter(|&p| p >= 12).unwrap_or(cutoff_byte);
    trimmed[..slice_end]
        .trim_end_matches(|c: char| matches!(c, ',' | '-' | '|' | ':' | '('))
        .trim()
        .to_string()
}

fn extract_product_jsonld(
    val: &Value,
    title: &mut String,
    desc: &mut String,
    price: &mut Option<f64>,
    currency: &mut String,
    sku: &mut String,
    mpn: &mut String,
) {
    match val {
        Value::Array(arr) => {
            for item in arr {
                extract_product_jsonld(item, title, desc, price, currency, sku, mpn);
            }
        }
        Value::Object(map) => {
            if let Some(graph) = map.get("@graph") {
                extract_product_jsonld(graph, title, desc, price, currency, sku, mpn);
            }
            let is_product = match map.get("@type") {
                Some(Value::String(s)) => s.eq_ignore_ascii_case("Product"),
                Some(Value::Array(arr)) => arr
                    .iter()
                    .any(|v| v.as_str().map(|s| s.eq_ignore_ascii_case("Product")).unwrap_or(false)),
                _ => false,
            };
            if is_product {
                if title.is_empty() {
                    if let Some(n) = map.get("name").and_then(|v| v.as_str()) {
                        *title = n.trim().to_string();
                    }
                }
                if desc.is_empty() {
                    if let Some(d) = map.get("description").and_then(|v| v.as_str()) {
                        *desc = d.trim().to_string();
                    }
                }
                if sku.is_empty() {
                    if let Some(s) = map.get("sku").and_then(|v| v.as_str()) {
                        *sku = s.trim().to_string();
                    }
                }
                if mpn.is_empty() {
                    if let Some(m) = map.get("mpn").and_then(|v| v.as_str()) {
                        *mpn = m.trim().to_string();
                    }
                }
                if let Some(offers) = map.get("offers") {
                    extract_offer_price(offers, price, currency);
                }
            }
        }
        _ => {}
    }
}

fn extract_offer_price(offers: &Value, price: &mut Option<f64>, currency: &mut String) {
    match offers {
        Value::Array(arr) => {
            for item in arr {
                if price.is_some() {
                    break;
                }
                extract_offer_price(item, price, currency);
            }
        }
        Value::Object(map) => {
            if price.is_none() {
                for key in ["price", "lowPrice"] {
                    if let Some(v) = map.get(key) {
                        if let Some(n) = v.as_f64() {
                            *price = Some(n);
                            break;
                        } else if let Some(s) = v.as_str() {
                            if let Some(n) = parse_price_str(s) {
                                *price = Some(n);
                                break;
                            }
                        }
                    }
                }
            }
            if currency.is_empty() {
                if let Some(c) = map.get("priceCurrency").and_then(|v| v.as_str()) {
                    *currency = c.trim().to_string();
                }
            }
        }
        _ => {}
    }
}

fn extract_meta_content(html: &str, key: &str) -> Option<String> {
    let escaped = regex::escape(key);
    let p1 = format!(
        r#"(?is)<meta[^>]+(?:property|name)\s*=\s*["']{}["'][^>]+content\s*=\s*["']([^"']*)["']"#,
        escaped
    );
    let p2 = format!(
        r#"(?is)<meta[^>]+content\s*=\s*["']([^"']*)["'][^>]+(?:property|name)\s*=\s*["']{}["']"#,
        escaped
    );
    capture_first(html, &p1)
        .or_else(|| capture_first(html, &p2))
        .map(|s| decode_html_entities(s.trim()))
        .filter(|s| !s.is_empty())
}

fn detect_supplier_name(host: &str) -> String {
    if host.contains("amazon.") || host == "a.co" {
        "Amazon".to_string()
    } else if host.contains("digikey.") {
        "DigiKey".to_string()
    } else if host.contains("mouser.") {
        "Mouser".to_string()
    } else if host.contains("lcsc.") {
        "LCSC".to_string()
    } else if host.contains("adafruit.") {
        "Adafruit".to_string()
    } else if host.contains("sparkfun.") {
        "SparkFun".to_string()
    } else if host.contains("mcmaster.") {
        "McMaster-Carr".to_string()
    } else if host.contains("aliexpress.") {
        "AliExpress".to_string()
    } else if host.contains("pololu.") {
        "Pololu".to_string()
    } else if host.contains("seeedstudio.") {
        "Seeed Studio".to_string()
    } else if host.contains("waveshare.") {
        "Waveshare".to_string()
    } else if host.contains("pimoroni.") {
        "Pimoroni".to_string()
    } else {
        let stripped = host.trim_start_matches("www.");
        let main = stripped.split('.').next().unwrap_or(stripped);
        let mut chars = main.chars();
        match chars.next() {
            None => "Supplier".to_string(),
            Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        }
    }
}

fn classify_part_category(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    if ["esp32", "rp2040", "stm32", "arduino", "teensy", "pico", "atmega", "microcontroller", "dev board", "feather"]
        .iter()
        .any(|k| lower.contains(k))
    {
        "microcontrollers".to_string()
    } else if ["sensor", "oled", "lcd", "tft", "e-ink", "eink", "gps", "imu", "accelerometer", "ultrasonic", "bme280", "ds18b20", "display", "camera", "lidar", "rfid"]
        .iter()
        .any(|k| lower.contains(k))
    {
        "sensors-modules".to_string()
    } else if ["connector", "jst", "xt60", "xt30", "usb-c", "header", "socket", "terminal", "crimp", "dupont", "pin header", "plug"]
        .iter()
        .any(|k| lower.contains(k))
    {
        "connectors".to_string()
    } else if ["resistor", "capacitor", "inductor", "mlcc", "diode", "mosfet", "transistor", "led ", "potentiometer", "crystal", "oscillator"]
        .iter()
        .any(|k| lower.contains(k))
    {
        "passives".to_string()
    } else {
        "hardware".to_string()
    }
}

fn detect_package(text: &str) -> String {
    let upper = text.to_ascii_uppercase();
    for pat in [
        r"\b(0402|0603|0805|1206|2512)\b",
        r"\b(SOT-23|SOT-223|TO-92|TO-220|TO-252)\b",
        r"\b(DIP-\d+|SOIC-\d+|SSOP-\d+|QFN-\d+|LQFP-\d+|TSSOP-\d+)\b",
    ] {
        if let Some(m) = capture_first(&upper, pat) {
            return m;
        }
    }
    let lower = text.to_ascii_lowercase();
    if lower.contains("breakout") || lower.contains("module") || lower.contains("board") {
        "Module".to_string()
    } else if lower.contains("smd") || lower.contains("smt") {
        "SMD".to_string()
    } else if lower.contains("through-hole") || lower.contains("axial") {
        "THT".to_string()
    } else {
        String::new()
    }
}

fn detect_pack_qty(title: &str) -> Option<i64> {
    for pat in [
        r"(?i)\bpack\s+of\s+(\d{1,4})\b",
        r"(?i)\b(\d{1,4})\s*[- ]?(?:pack|pcs|pieces)\b",
    ] {
        if let Some(m) = capture_first(title, pat) {
            if let Ok(n) = m.parse::<i64>() {
                if (2..=1000).contains(&n) {
                    return Some(n);
                }
            }
        }
    }
    None
}

fn clean_store_title(raw: &str) -> String {
    let mut s = raw.trim().to_string();
    for prefix in ["Amazon.com: ", "Amazon.com : "] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest.to_string();
        }
    }
    for suffix_pat in [
        r"(?i)\s*[-:|]\s*Amazon\.(?:com|ca|co\.uk|de)\b.*$",
        r"(?i)\s*:\s*ID\s+\d+\s*:\s*Adafruit.*$",
        r"(?i)\s*[-|]\s*(?:Adafruit|SparkFun|DigiKey|Mouser|LCSC).*$",
        r"(?i)\s*:\s*(?:Electronics|Industrial & Scientific|Tools & Home Improvement).*$",
    ] {
        if let Ok(re) = Regex::new(suffix_pat) {
            s = re.replace(&s, "").to_string();
        }
    }
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn parse_price_str(s: &str) -> Option<f64> {
    let re = Regex::new(r"([0-9][0-9,]*\.?\d*)").ok()?;
    let m = re.captures(s)?;
    let num_str = m[1].replace(',', "");
    num_str.parse::<f64>().ok().filter(|&v| v > 0.0)
}

fn capture_first(text: &str, pattern: &str) -> Option<String> {
    let re = Regex::new(pattern).ok()?;
    let cap = re.captures(text)?;
    cap.get(1).map(|m| m.as_str().to_string())
}

fn strip_tags(raw: &str) -> String {
    let re = Regex::new(r"(?is)<[^>]+>").unwrap();
    re.replace_all(raw, " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn decode_html_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#34;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
        .replace("&ndash;", "-")
        .replace("&mdash;", "-")
}

fn truncate_clean(s: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (idx, ch) in s.chars().enumerate() {
        if idx >= max_chars {
            out.push_str("...");
            break;
        }
        out.push(ch);
    }
    out
}

pub fn extract_gallery_images(html: &str, base_url: &str) -> Vec<CandidateImage> {
    let mut candidates: Vec<(String, String)> = Vec::new(); // (url, context_hint)

    // 1. Amazon colorImages hiRes / large URLs (returns all gallery photos + pinout diagrams)
    for pat in [
        r#"(?is)"hiRes"\s*:\s*"(https?://[^"]+)""#,
        r#"(?is)"large"\s*:\s*"(https?://[^"]+)""#,
    ] {
        if let Ok(re) = Regex::new(pat) {
            for cap in re.captures_iter(html) {
                candidates.push((cap[1].replace("\\/", "/"), "gallery".to_string()));
            }
        }
    }

    // 2. JSON-LD Product "image"
    if let Ok(re_ld) = Regex::new(r#"(?is)<script[^>]*type\s*=\s*["']application/ld\+json["'][^>]*>(.*?)</script>"#) {
        for cap in re_ld.captures_iter(html) {
            if let Ok(val) = serde_json::from_str::<Value>(cap[1].trim()) {
                collect_jsonld_images(&val, &mut candidates);
            }
        }
    }

    // 3. OpenGraph / Twitter image meta tags
    for key in ["og:image", "og:image:secure_url", "twitter:image"] {
        if let Some(u) = extract_meta_content(html, key) {
            candidates.push((u, key.to_string()));
        }
    }

    // 4. HTML <img> tags fallback (skip sprites/logos/pixels)
    if let Ok(re_img) = Regex::new(r#"(?is)<img\b([^>]+)>"#) {
        for cap in re_img.captures_iter(html) {
            let attrs = &cap[1];
            let src = capture_first(attrs, r#"(?is)(?:data-old-hires|data-src|src)\s*=\s*["']([^"']+)["']"#);
            let alt = capture_first(attrs, r#"(?is)alt\s*=\s*["']([^"']*)["']"#).unwrap_or_default();
            if let Some(raw_src) = src {
                candidates.push((raw_src, alt));
            }
        }
    }

    let base_parsed = reqwest::Url::parse(base_url).ok();
    let mut seen_keys: HashSet<String> = HashSet::new();
    let mut out: Vec<CandidateImage> = Vec::new();

    for (raw_url, hint) in candidates {
        let decoded = decode_html_entities(raw_url.trim());
        if decoded.is_empty() || decoded.starts_with("data:") {
            continue;
        }
        let abs_url = if decoded.starts_with("http://") || decoded.starts_with("https://") {
            decoded
        } else if let Some(ref base) = base_parsed {
            match base.join(&decoded) {
                Ok(u) => u.to_string(),
                Err(_) => continue,
            }
        } else {
            continue;
        };

        let lower_u = abs_url.to_ascii_lowercase();
        if !lower_u.contains(".jpg")
            && !lower_u.contains(".jpeg")
            && !lower_u.contains(".png")
            && !lower_u.contains(".webp")
        {
            continue;
        }
        if [
            "sprite", "favicon", "1x1", "pixel", "transparent", "blank.gif",
            "play-button", "badge", "avatar", "spinner", "loading", "icon-",
            "grey-pixel", "nav-sprite",
        ]
        .iter()
        .any(|bad| lower_u.contains(bad))
        {
            continue;
        }

        // Deduplicate Amazon resize variants like /I/71abc._AC_SX679_.jpg -> /I/71abc
        let dedup_key = if let Some(m) = capture_first(&abs_url, r"(?i)(/images/I/[A-Za-z0-9+_-]+)\.") {
            m
        } else {
            abs_url.split('?').next().unwrap_or(&abs_url).to_string()
        };
        if !seen_keys.insert(dedup_key) {
            continue;
        }

        let combined_hint = format!("{} {}", lower_u, hint.to_ascii_lowercase());
        let likely_pinout = [
            "pinout", "pin-out", "gpio", "diagram", "schematic", "dimension",
            "layout", "wiring", "pin_map", "pinmap", "interface", "footprint",
        ]
        .iter()
        .any(|kw| combined_hint.contains(kw));

        let label = if likely_pinout {
            "Possible Pinout / Diagram".to_string()
        } else if out.is_empty() {
            "Primary Image".to_string()
        } else {
            format!("Image {}", out.len() + 1)
        };

        out.push(CandidateImage {
            url: abs_url,
            label,
            likely_pinout,
        });
        if out.len() >= 12 {
            break;
        }
    }

    out
}

fn collect_jsonld_images(val: &Value, out: &mut Vec<(String, String)>) {
    match val {
        Value::Array(arr) => {
            for item in arr {
                collect_jsonld_images(item, out);
            }
        }
        Value::Object(map) => {
            if let Some(graph) = map.get("@graph") {
                collect_jsonld_images(graph, out);
            }
            if let Some(img) = map.get("image") {
                match img {
                    Value::String(s) => out.push((s.clone(), "jsonld".to_string())),
                    Value::Array(arr) => {
                        for el in arr {
                            if let Some(s) = el.as_str() {
                                out.push((s.to_string(), "jsonld".to_string()));
                            } else if let Some(u) = el.get("url").and_then(|v| v.as_str()) {
                                out.push((u.to_string(), "jsonld".to_string()));
                            }
                        }
                    }
                    Value::Object(obj) => {
                        if let Some(u) = obj.get("url").and_then(|v| v.as_str()) {
                            out.push((u.to_string(), "jsonld".to_string()));
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

pub async fn fetch_proxy_image_bytes(raw_url: &str) -> Result<(Vec<u8>, &'static str)> {
    let (clean_url, _) = validate_public_http_url(raw_url)?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(Policy::limited(5))
        .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36")
        .build()?;
    let resp = client.get(&clean_url).send().await?;
    if !resp.status().is_success() {
        return Err(anyhow!("Image fetch returned HTTP {}", resp.status().as_u16()));
    }
    let bytes = resp.bytes().await?;
    if bytes.len() > 10 * 1024 * 1024 {
        return Err(anyhow!("Image exceeds 10 MB limit"));
    }
    let fmt = image::guess_format(&bytes)
        .map_err(|_| anyhow!("Downloaded file is not a supported image"))?;
    let mime = match fmt {
        image::ImageFormat::Png => "image/png",
        image::ImageFormat::Jpeg => "image/jpeg",
        image::ImageFormat::WebP => "image/webp",
        image::ImageFormat::Gif => "image/gif",
        _ => "application/octet-stream",
    };
    Ok((bytes.to_vec(), mime))
}

pub async fn download_and_save_image(raw_url: &str, sku: &str, kind: &str, data_dir: &Path) -> Result<String> {
    let (bytes, _) = fetch_proxy_image_bytes(raw_url).await?;
    save_uploaded_image(&bytes, sku, kind, data_dir)
}

pub fn save_uploaded_image(bytes: &[u8], sku: &str, kind: &str, data_dir: &Path) -> Result<String> {
    if bytes.is_empty() || bytes.len() > 10 * 1024 * 1024 {
        return Err(anyhow!("Image must be between 1 byte and 10 MB"));
    }
    let fmt = image::guess_format(bytes)
        .map_err(|_| anyhow!("Unsupported or invalid image format"))?;
    let ext = match fmt {
        image::ImageFormat::Png => "png",
        image::ImageFormat::Jpeg => "jpg",
        image::ImageFormat::WebP => "webp",
        image::ImageFormat::Gif => "gif",
        _ => return Err(anyhow!("Only PNG, JPEG, WebP, and GIF images are supported")),
    };
    let safe_sku: String = sku
        .trim()
        .to_uppercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let safe_kind = if kind.eq_ignore_ascii_case("pinout") { "pinout" } else { "part" };
    let filename = format!("{}_{}.{}", safe_sku, safe_kind, ext);
    let images_dir = data_dir.join("images");
    fs::create_dir_all(&images_dir)?;
    // Remove older extensions for the same sku+kind
    for old_ext in ["png", "jpg", "webp", "gif"] {
        if old_ext != ext {
            let _ = fs::remove_file(images_dir.join(format!("{}_{}.{}", safe_sku, safe_kind, old_ext)));
        }
    }
    fs::write(images_dir.join(&filename), bytes)?;
    Ok(filename)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssrf_guard_blocks_local_and_private_urls() {
        assert!(validate_public_http_url("http://127.0.0.1:8090/").is_err());
        assert!(validate_public_http_url("http://localhost:8000/").is_err());
        assert!(validate_public_http_url("http://192.168.1.50/").is_err());
        assert!(validate_public_http_url("http://10.0.0.1/").is_err());
        assert!(validate_public_http_url("http://169.254.169.254/latest/meta-data/").is_err());
        assert!(validate_public_http_url("file:///etc/passwd").is_err());
        assert!(validate_public_http_url("https://www.adafruit.com/product/5400").is_ok());
    }

    #[test]
    fn parses_amazon_url_and_pack_pricing() {
        let db = Database::open_in_memory().unwrap();
        let url = "https://www.amazon.com/3PCS-ESP32-S3-DevKitC-Development-Board/dp/B0CKXJLP4B/ref=sr_1_3?keywords=esp32-s3&qid=1795822753&sr=8-3";
        let mut draft = base_draft_from_url(url, "www.amazon.com");
        assert_eq!(draft.supplier_name, "Amazon");
        assert_eq!(draft.supplier_sku, "B0CKXJLP4B");

        let html = r#"
            <html><head><title>Amazon.com: 3PCS ESP32 ESP32-S3-DevKitC-1 N16R8 Development Board WiFi Bluetooth Dual Type-C, 16MB Flash 8MB PSRAM for Arduino : Electronics</title></head>
            <body>
              <span id="productTitle">  3PCS ESP32 ESP32-S3-DevKitC-1 N16R8 Development Board WiFi Bluetooth Dual Type-C, 16MB Flash 8MB PSRAM for Arduino </span>
              <span class="a-price"><span class="a-offscreen">$18.99</span></span>
            </body></html>
        "#;
        enrich_draft_from_html(&mut draft, html);
        finalize_draft(&mut draft, &db).unwrap();

        assert_eq!(draft.url, "https://www.amazon.com/dp/B0CKXJLP4B");
        assert_eq!(draft.category, "microcontrollers");
        assert_eq!(draft.suggested_sku, "MCU-001");
        assert_eq!(draft.pack_qty, 3);
        assert_eq!(draft.total_price, 18.99);
        assert_eq!(draft.unit_cost, 6.33);
        assert!(!draft.name.starts_with("3PCS"));
        assert!(!draft.name.contains("ESP32 ESP32-S3"));
        assert!(draft.name.len() <= 56);
        assert!(draft.spec.contains("16MB Flash"));
    }

    #[test]
    fn parses_jsonld_product_metadata() {
        let db = Database::open_in_memory().unwrap();
        let url = "https://www.adafruit.com/product/5691";
        let mut draft = base_draft_from_url(url, "www.adafruit.com");
        let html = r#"
            <html><head>
              <script type="application/ld+json">
              {
                "@context": "https://schema.org",
                "@type": "Product",
                "name": "Adafruit BME280 I2C or SPI Temperature Humidity Pressure Sensor",
                "description": "Precision environmental sensor module with STEMMA QT",
                "sku": "2652",
                "mpn": "ADA-2652",
                "offers": { "@type": "Offer", "price": "14.95", "priceCurrency": "USD" }
              }
              </script>
            </head><body></body></html>
        "#;
        enrich_draft_from_html(&mut draft, html);
        finalize_draft(&mut draft, &db).unwrap();

        assert_eq!(draft.supplier_name, "Adafruit");
        assert_eq!(draft.category, "sensors-modules");
        assert_eq!(draft.suggested_sku, "SEN-001");
        assert_eq!(draft.mpn, "ADA-2652");
        assert_eq!(draft.unit_cost, 14.95);
        assert_eq!(draft.package, "Module");
        assert!(draft.description.contains("Precision environmental sensor module with STEMMA QT"));
    }

    #[test]
    fn extracts_amazon_feature_bullets_and_skips_boilerplate() {
        let db = Database::open_in_memory().unwrap();
        let url = "https://www.amazon.com/dp/B0CKXJLP4B";
        let mut draft = base_draft_from_url(url, "www.amazon.com");
        let html = r#"
            <html><body>
              <span id="productTitle">ESP32-S3-DevKitC-1 N16R8 Board</span>
              <div id="feature-bullets">
                <ul>
                  <li><span class="a-list-item">Make sure this fits by entering your model number.</span></li>
                  <li><span class="a-list-item">16MB Flash and 8MB OPI PSRAM onboard</span></li>
                  <li><span class="a-list-item">Dual USB-C ports with native USB-OTG and CH343P UART</span></li>
                  <li><span class="a-list-item">See more product details</span></li>
                </ul>
              </div>
              <div id="productDescription"><p>Compact WiFi + BLE 5.0 IoT development board.</p></div>
            </body></html>
        "#;
        enrich_draft_from_html(&mut draft, html);
        finalize_draft(&mut draft, &db).unwrap();

        assert!(draft.description.contains("- 16MB Flash and 8MB OPI PSRAM onboard"));
        assert!(draft.description.contains("- Dual USB-C ports with native USB-OTG and CH343P UART"));
        assert!(draft.description.contains("Compact WiFi + BLE 5.0 IoT development board."));
        assert!(!draft.description.contains("Make sure this fits"));
        assert!(!draft.description.contains("See more product details"));
    }

    #[test]
    fn detects_lcsc_sku_and_antibot_gracefully() {
        let draft = base_draft_from_url("https://www.lcsc.com/product-detail/C123456.html", "www.lcsc.com");
        assert_eq!(draft.supplier_name, "LCSC");
        assert_eq!(draft.supplier_sku, "C123456");

        let captcha_html = "<html><title>Robot Check</title><form action='/errors/validateCaptcha'>Type the characters you see in this image</form></html>";
        assert!(is_antibot_or_captcha(200, captcha_html));
        assert!(is_antibot_or_captcha(403, "<html><body>Just a moment...</body></html>"));

        let amz_soft_shell = r#"<!DOCTYPE html><html class="a-no-js" lang="en-us"><head><meta charset="utf-8"></head><body></body></html>"#;
        assert!(is_antibot_or_captcha(200, amz_soft_shell));
    }

    #[test]
    fn extracts_amazon_gallery_and_pinout_hints() {
        let html = r#"
            <script>
            var data = {
              "colorImages": { "initial": [
                {"hiRes":"https://m.media-amazon.com/images/I/71BoardMain._AC_SL1500_.jpg","large":"https://m.media-amazon.com/images/I/71BoardMain._AC_SX500_.jpg"},
                {"hiRes":"https://m.media-amazon.com/images/I/71ESP32-pinout-diagram._AC_SL1500_.png"}
              ]}
            };
            </script>
        "#;
        let imgs = extract_gallery_images(html, "https://www.amazon.com/dp/B0CKXJLP4B");
        assert_eq!(imgs.len(), 2);
        assert!(!imgs[0].likely_pinout);
        assert!(imgs[1].likely_pinout);
    }
}
