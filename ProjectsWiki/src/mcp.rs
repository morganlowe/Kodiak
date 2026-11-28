use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::Path;

use crate::barcodes::decode::decode_image_bytes;
use crate::barcodes::labels::{
    build_bin_label, build_part_label, render_label_svg, BarcodeStyle, LabelPreset,
};
use crate::barcodes::pdf::render_labels_pdf;
use crate::barcodes::phomemo;
use crate::config::AppConfig;
use crate::db::Database;
use crate::export;
use crate::models::*;
use crate::scrape;

pub fn run_mcp_stdio(cfg: &AppConfig, db: &Database) -> Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                let err = json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": { "code": -32700, "message": format!("Parse error: {}", e) }
                });
                writeln!(stdout, "{}", err)?;
                stdout.flush()?;
                continue;
            }
        };

        if let Some(resp) = handle_request(&msg, cfg, db) {
            writeln!(stdout, "{}", resp)?;
            stdout.flush()?;
        }
    }
    Ok(())
}

pub fn handle_request(msg: &Value, cfg: &AppConfig, db: &Database) -> Option<Value> {
    let id = msg.get("id").cloned();
    let method = msg.get("method").and_then(|v| v.as_str()).unwrap_or("");
    let params = msg.get("params").cloned().unwrap_or_else(|| json!({}));

    match method {
        "initialize" => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "benchbear-mcp", "version": env!("CARGO_PKG_VERSION") }
            }
        })),
        "notifications/initialized" => None,
        "tools/list" => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": { "tools": tool_definitions() }
        })),
        "tools/call" => {
            let tname = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            let res = match dispatch_tool(tname, &args, cfg, db) {
                Ok(val) => json!({
                    "content": [{ "type": "text", "text": serde_json::to_string_pretty(&val).unwrap_or_default() }]
                }),
                Err(e) => json!({
                    "content": [{ "type": "text", "text": format!("Error: {}", e) }],
                    "isError": true
                }),
            };
            Some(json!({ "jsonrpc": "2.0", "id": id, "result": res }))
        }
        _ => id.map(|req_id| {
            json!({
                "jsonrpc": "2.0",
                "id": req_id,
                "error": { "code": -32601, "message": format!("Method not found: {}", method) }
            })
        }),
    }
}

fn dispatch_tool(name: &str, args: &Value, cfg: &AppConfig, db: &Database) -> Result<Value> {
    match name {
        "search_parts" => {
            let q = args.get("query").and_then(|v| v.as_str());
            let cat = args.get("category").and_then(|v| v.as_str());
            let loc = args.get("location").and_then(|v| v.as_str());
            let sup = args.get("supplier").and_then(|v| v.as_str());
            let low = args.get("low_stock_only").and_then(|v| v.as_bool()).unwrap_or(false);
            let sort = args.get("sort").and_then(|v| v.as_str());
            Ok(serde_json::to_value(db.list_parts_sorted(q, cat, loc, sup, low, sort)?)?)
        }
        "get_part" => {
            let sku = req_str(args, "sku")?;
            let detail = db
                .get_part_detail(sku)?
                .ok_or_else(|| anyhow!("SKU not found: {}", sku))?;
            Ok(serde_json::to_value(detail)?)
        }
        "add_part" | "update_part" => {
            let req: UpsertPart = serde_json::from_value(args.clone())?;
            let part = db.upsert_part(&req)?;
            let _ = export::sync_all(db, cfg);
            Ok(serde_json::to_value(part)?)
        }
        "adjust_stock" => {
            let sku = req_str(args, "sku")?;
            let delta = args.get("delta").and_then(|v| v.as_i64()).ok_or_else(|| anyhow!("delta is required"))?;
            let reason = args.get("reason").and_then(|v| v.as_str()).unwrap_or("adjust").to_string();
            let note = args.get("note").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let unit_cost = args.get("unit_cost").and_then(|v| v.as_f64());
            let supplier_id = args.get("supplier_id").and_then(|v| v.as_i64());
            let project_slug = args.get("project_slug").and_then(|v| v.as_str()).map(|s| s.to_string());
            let part = db.adjust_stock(sku, &AdjustStockReq { delta, reason, note, unit_cost, supplier_id, project_slug })?;
            let _ = export::sync_all(db, cfg);
            Ok(serde_json::to_value(part)?)
        }
        "move_part" => {
            let sku = req_str(args, "sku")?;
            let location_code = req_str(args, "location_code")?.to_string();
            let note = args.get("note").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let part = db.move_part(sku, &MovePartReq { location_code, note })?;
            let _ = export::sync_all(db, cfg);
            Ok(serde_json::to_value(part)?)
        }
        "list_locations" => Ok(serde_json::to_value(db.list_locations()?)?),
        "upsert_location" => {
            let req: UpsertLocation = serde_json::from_value(args.clone())?;
            let loc = db.upsert_location(&req)?;
            let _ = export::sync_all(db, cfg);
            Ok(serde_json::to_value(loc)?)
        }
        "list_suppliers" => Ok(serde_json::to_value(db.list_suppliers()?)?),
        "upsert_supplier" => {
            let req: UpsertSupplier = serde_json::from_value(args.clone())?;
            let sup = db.upsert_supplier(&req)?;
            let _ = export::sync_all(db, cfg);
            Ok(serde_json::to_value(sup)?)
        }
        "add_part_supplier" => {
            let sku = req_str(args, "sku")?;
            let req: UpsertPartSupplier = serde_json::from_value(args.clone())?;
            let rows = db.add_part_supplier(sku, &req)?;
            let _ = export::sync_all(db, cfg);
            Ok(serde_json::to_value(rows)?)
        }
        "get_valuation" => Ok(serde_json::to_value(db.compute_valuation()?)?),
        "get_reorder_list" => Ok(serde_json::to_value(db.compute_reorder_list()?)?),
        "read_project" => {
            let name = req_str(args, "name")?;
            let slug = if name.ends_with(".md") { name.to_string() } else { format!("{}.md", name) };
            let target = cfg.projects_dir.join(&slug);
            let canonical_proj = fs::canonicalize(&cfg.projects_dir)?;
            let canonical_target = fs::canonicalize(&target)?;
            if !canonical_target.starts_with(&canonical_proj) {
                return Err(anyhow!("Path outside projects directory"));
            }
            Ok(json!({
                "project": slug,
                "content": fs::read_to_string(canonical_target)?
            }))
        }
        "check_bom" => {
            let proj = req_str(args, "project_name")?;
            let clean = proj.trim_end_matches(".md");
            let boms = db.list_boms()?;
            let found = boms
                .into_iter()
                .find(|b| b.slug.eq_ignore_ascii_case(clean))
                .ok_or_else(|| anyhow!("BOM not found: {}", proj))?;
            Ok(serde_json::to_value(found)?)
        }
        "deduct_bom" => {
            let slug = req_str(args, "project_name")?;
            let mult = args.get("multiplier").and_then(|v| v.as_i64()).unwrap_or(1);
            let note = args.get("note").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let bom = db.deduct_bom(slug, &DeductBomReq { multiplier: mult, note })?;
            let _ = export::sync_all(db, cfg);
            Ok(serde_json::to_value(bom)?)
        }
        "generate_label" => {
            let kind = args.get("kind").and_then(|v| v.as_str()).unwrap_or("part");
            let id = req_str(args, "id")?;
            let barcode = BarcodeStyle::from_str(args.get("barcode").and_then(|v| v.as_str()).unwrap_or("combo"));
            let w_mm = args.get("w_mm").and_then(|v| v.as_f64());
            let h_mm = args.get("h_mm").and_then(|v| v.as_f64());
            let w_in = args.get("w_in").and_then(|v| v.as_f64());
            let h_in = args.get("h_in").and_then(|v| v.as_f64());
            let preset = LabelPreset::from_dimensions(
                Some(args.get("preset").and_then(|v| v.as_str()).unwrap_or(&cfg.default_label_preset)),
                w_mm,
                h_mm,
                w_in,
                h_in,
            );
            let fmt = args.get("format").and_then(|v| v.as_str()).unwrap_or("pdf").to_ascii_lowercase();

            let spec = if kind.eq_ignore_ascii_case("bin") || kind.eq_ignore_ascii_case("location") {
                let loc = db.get_location(id)?.ok_or_else(|| anyhow!("Location not found: {}", id))?;
                let parts = db.list_parts(None, None, Some(&loc.location.code), None, false)?;
                build_bin_label(&loc, &parts, preset, barcode)?
            } else {
                let part = db.get_part(id)?.ok_or_else(|| anyhow!("SKU not found: {}", id))?;
                build_part_label(&part, preset, barcode)?
            };

            let labels_dir = cfg.data_dir.join("labels");
            fs::create_dir_all(&labels_dir)?;
            let safe_id = id.replace(['/', '\\', ' '], "_");
            let ext = match fmt.as_str() {
                "svg" => "svg",
                "png" => "png",
                "phomemo" => "phomemo",
                _ => "pdf",
            };
            let w_out = spec.width_mm;
            let h_out = spec.height_mm;
            let out_path = labels_dir.join(format!("{}_{}.{}", kind, safe_id, ext));
            if ext == "svg" {
                fs::write(&out_path, render_label_svg(&spec))?;
            } else if ext == "png" {
                fs::write(&out_path, phomemo::render_label_png(&spec, phomemo::PHOMEMO_DPI)?)?;
            } else if ext == "phomemo" {
                let luma = phomemo::rasterize_label_spec_to_luma(&spec, phomemo::PHOMEMO_DPI);
                let pkt = phomemo::encode_m110_packet(&luma, 10, "gap", 1);
                fs::write(&out_path, pkt)?;
            } else {
                fs::write(&out_path, render_labels_pdf(&[spec])?)?;
            }
            Ok(json!({ "path": out_path, "format": ext, "width_mm": w_out, "height_mm": h_out }))
        }
        "decode_barcode" => {
            let path = Path::new(req_str(args, "image_path")?);
            let bytes = fs::read(path)?;
            let decoded = decode_image_bytes(&bytes, db)?;
            Ok(serde_json::to_value(decoded)?)
        }
        "fetch_product_url" => {
            let url = req_str(args, "url")?;
            let draft = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(scrape::fetch_product_url(url, db))
            })?;
            Ok(serde_json::to_value(draft)?)
        }
        _ => Err(anyhow!("Unknown tool: {}", name)),
    }
}

fn req_str<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("{} is required", key))
}

fn tool_definitions() -> Vec<Value> {
    vec![
        json!({"name":"search_parts","description":"Search inventory parts by query (SKU, name, spec, description, notes, MPN, supplier, location, projects), category, location, supplier, or low-stock status, with sorting by date added/modified/etc.","inputSchema":{"type":"object","properties":{"query":{"type":"string"},"category":{"type":"string"},"location":{"type":"string"},"supplier":{"type":"string"},"low_stock_only":{"type":"boolean"},"sort":{"type":"string","enum":["category_sku","sku_asc","created_desc","created_asc","updated_desc","updated_asc","name_asc","qty_asc","qty_desc","value_desc"]}}}}),
        json!({"name":"get_part","description":"Get full part details, alternate suppliers, and stock movement history","inputSchema":{"type":"object","required":["sku"],"properties":{"sku":{"type":"string"}}}}),
        json!({"name":"add_part","description":"Create or update a part with location, supplier, MPN, unit cost, description, notes, and image URLs","inputSchema":{"type":"object","required":["sku","category","name"],"properties":{"sku":{"type":"string"},"category":{"type":"string"},"name":{"type":"string"},"spec":{"type":"string"},"package":{"type":"string"},"qty":{"type":"integer"},"min_qty":{"type":"integer"},"unit_cost":{"type":"number"},"location_code":{"type":"string"},"supplier_name":{"type":"string"},"mpn":{"type":"string"},"supplier_sku":{"type":"string"},"product_url":{"type":"string"},"datasheet_url":{"type":"string"},"projects_tags":{"type":"string"},"description":{"type":"string"},"notes":{"type":"string"},"image_url":{"type":"string"},"pinout_image_url":{"type":"string"}}}}),
        json!({"name":"update_part","description":"Update an existing part's fields including description, notes, and images","inputSchema":{"type":"object","required":["sku","category","name"],"properties":{"sku":{"type":"string"},"category":{"type":"string"},"name":{"type":"string"},"spec":{"type":"string"},"package":{"type":"string"},"qty":{"type":"integer"},"min_qty":{"type":"integer"},"unit_cost":{"type":"number"},"location_code":{"type":"string"},"supplier_name":{"type":"string"},"mpn":{"type":"string"},"supplier_sku":{"type":"string"},"product_url":{"type":"string"},"datasheet_url":{"type":"string"},"projects_tags":{"type":"string"},"description":{"type":"string"},"notes":{"type":"string"},"image_url":{"type":"string"},"pinout_image_url":{"type":"string"}}}}),
        json!({"name":"adjust_stock","description":"Adjust part quantity with optional restock unit cost and movement note","inputSchema":{"type":"object","required":["sku","delta"],"properties":{"sku":{"type":"string"},"delta":{"type":"integer"},"reason":{"type":"string"},"note":{"type":"string"},"unit_cost":{"type":"number"},"supplier_id":{"type":"integer"},"project_slug":{"type":"string"}}}}),
        json!({"name":"move_part","description":"Move a part to a different bin/location","inputSchema":{"type":"object","required":["sku","location_code"],"properties":{"sku":{"type":"string"},"location_code":{"type":"string"},"note":{"type":"string"}}}}),
        json!({"name":"list_locations","description":"List all storage bins/locations with SKU count, unit count, and value","inputSchema":{"type":"object","properties":{}}}),
        json!({"name":"upsert_location","description":"Create or update a bin/location and its barcode","inputSchema":{"type":"object","required":["code"],"properties":{"code":{"type":"string"},"name":{"type":"string"},"area":{"type":"string"},"rack":{"type":"string"},"drawer":{"type":"string"},"description":{"type":"string"}}}}),
        json!({"name":"list_suppliers","description":"List all suppliers/vendors with part counts and stocked value","inputSchema":{"type":"object","properties":{}}}),
        json!({"name":"upsert_supplier","description":"Create or update a supplier/vendor","inputSchema":{"type":"object","required":["name"],"properties":{"name":{"type":"string"},"website":{"type":"string"},"currency":{"type":"string"},"lead_time_days":{"type":"integer"},"shipping_notes":{"type":"string"}}}}),
        json!({"name":"add_part_supplier","description":"Attach or update a supplier quote/SKU/MPN for a part","inputSchema":{"type":"object","required":["sku"],"properties":{"sku":{"type":"string"},"supplier_name":{"type":"string"},"supplier_sku":{"type":"string"},"mpn":{"type":"string"},"unit_cost":{"type":"number"},"moq":{"type":"integer"},"product_url":{"type":"string"},"is_preferred":{"type":"boolean"}}}}),
        json!({"name":"get_valuation","description":"Get inventory valuation and restock cost rollup by category, location, and supplier","inputSchema":{"type":"object","properties":{}}}),
        json!({"name":"get_reorder_list","description":"Get low-stock parts grouped by supplier with estimated order totals","inputSchema":{"type":"object","properties":{}}}),
        json!({"name":"read_project","description":"Read a project Markdown page and BOM","inputSchema":{"type":"object","required":["name"],"properties":{"name":{"type":"string"}}}}),
        json!({"name":"check_bom","description":"Check inventory availability, bin locations, and cost rollup for a project BOM","inputSchema":{"type":"object","required":["project_name"],"properties":{"project_name":{"type":"string"}}}}),
        json!({"name":"deduct_bom","description":"Atomically deduct all parts for a project BOM build","inputSchema":{"type":"object","required":["project_name"],"properties":{"project_name":{"type":"string"},"multiplier":{"type":"integer"},"note":{"type":"string"}}}}),
        json!({"name":"generate_label","description":"Generate a thermal SVG, vector PDF, 203-DPI PNG, or raw Phomemo M110 ESC/POS file for a part or bin (supports presets or custom w_mm/h_mm or w_in/h_in)","inputSchema":{"type":"object","required":["kind","id"],"properties":{"kind":{"type":"string","enum":["part","bin"]},"id":{"type":"string"},"barcode":{"type":"string","enum":["combo","code128","qr"]},"preset":{"type":"string","enum":["2x1","2.25x1.25","ql62x29","ql62x42","m110_40x30","m110_50x30","m110_40x20","m110_30x20","m110_50x80"]},"w_mm":{"type":"number"},"h_mm":{"type":"number"},"w_in":{"type":"number"},"h_in":{"type":"number"},"format":{"type":"string","enum":["pdf","svg","png","phomemo"]}}}}),
        json!({"name":"decode_barcode","description":"Decode 1D/2D barcodes from an image file and resolve matching part or bin","inputSchema":{"type":"object","required":["image_path"],"properties":{"image_path":{"type":"string"}}}}),
        json!({"name":"fetch_product_url","description":"Fetch and extract part name, spec, description, price, supplier, SKU, and gallery/pinout image metadata from an Amazon/Adafruit/SparkFun/LCSC/DigiKey or general product URL","inputSchema":{"type":"object","required":["url"],"properties":{"url":{"type":"string"}}}}),
    ]
}
