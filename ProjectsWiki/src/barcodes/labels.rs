use anyhow::Result;

use crate::barcodes::code128::{encode_code128b, Code128Encoded};
use crate::barcodes::qr::{encode_qr, QrMatrix};
use crate::models::{LabelPresetInfo, LocationWithStats, Part};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelPreset {
    In2x1,
    In225x125,
    Ql62x29,
    Ql62x42,
    Phomemo40x30,
    Phomemo50x30,
    Phomemo40x20,
    Phomemo30x20,
    Phomemo50x80,
    CustomMm10(u16, u16),
}

impl LabelPreset {
    pub fn custom_mm(w_mm: f64, h_mm: f64) -> Self {
        let w10 = (w_mm * 10.0).round().clamp(150.0, 1500.0) as u16;
        let h10 = (h_mm * 10.0).round().clamp(150.0, 1500.0) as u16;
        Self::CustomMm10(w10, h10)
    }

    pub fn from_dimensions(
        preset: Option<&str>,
        w_mm: Option<f64>,
        h_mm: Option<f64>,
        w_in: Option<f64>,
        h_in: Option<f64>,
    ) -> Self {
        if let (Some(w), Some(h)) = (w_mm, h_mm) {
            if w > 0.0 && h > 0.0 {
                return Self::custom_mm(w, h);
            }
        }
        if let (Some(wi), Some(hi)) = (w_in, h_in) {
            if wi > 0.0 && hi > 0.0 {
                return Self::custom_mm(wi * 25.4, hi * 25.4);
            }
        }
        Self::from_str(preset.unwrap_or("2x1"))
    }

    pub fn from_str(s: &str) -> Self {
        let norm = s.trim().to_ascii_lowercase();
        if let Some(rest) = norm.strip_prefix("custom_in:") {
            if let Some((w_s, h_s)) = rest.split_once('x') {
                if let (Ok(wi), Ok(hi)) = (w_s.trim().parse::<f64>(), h_s.trim().parse::<f64>()) {
                    if wi > 0.0 && hi > 0.0 {
                        return Self::custom_mm(wi * 25.4, hi * 25.4);
                    }
                }
            }
        }
        if let Some(rest) = norm.strip_prefix("custom:") {
            if let Some((w_s, h_s)) = rest.split_once('x') {
                if let (Ok(w), Ok(h)) = (w_s.trim().parse::<f64>(), h_s.trim().parse::<f64>()) {
                    if w > 0.0 && h > 0.0 {
                        return Self::custom_mm(w, h);
                    }
                }
            }
        }
        match norm.as_str() {
            "2.25x1.25" | "225x125" => Self::In225x125,
            "ql62x29" | "62x29" | "dk1209" => Self::Ql62x29,
            "ql62x42" | "62x42" => Self::Ql62x42,
            "m110_40x30" | "40x30" | "phomemo_40x30" => Self::Phomemo40x30,
            "m110_50x30" | "50x30" | "phomemo_50x30" => Self::Phomemo50x30,
            "m110_40x20" | "40x20" | "phomemo_40x20" => Self::Phomemo40x20,
            "m110_30x20" | "30x20" | "phomemo_30x20" => Self::Phomemo30x20,
            "m110_50x80" | "50x80" | "phomemo_50x80" => Self::Phomemo50x80,
            _ => Self::In2x1,
        }
    }

    pub fn info(&self) -> LabelPresetInfo {
        let (id, name, w_mm, h_mm, desc): (String, String, f64, f64, String) = match self {
            Self::In2x1 => (
                "2x1".into(),
                "2\" x 1\" Thermal Roll".into(),
                50.8,
                25.4,
                "Standard 2x1 inch thermal part & drawer label".into(),
            ),
            Self::In225x125 => (
                "2.25x1.25".into(),
                "2.25\" x 1.25\" Thermal Roll".into(),
                57.15,
                31.75,
                "Medium thermal label with room for QR + Code 128 + specs".into(),
            ),
            Self::Ql62x29 => (
                "ql62x29".into(),
                "Brother QL 62mm x 29mm".into(),
                62.0,
                29.0,
                "Brother QL DK-1209 die-cut address/part label".into(),
            ),
            Self::Ql62x42 => (
                "ql62x42".into(),
                "Brother QL 62mm x 42mm Bin".into(),
                62.0,
                42.0,
                "Large bin label with contents list, QR, and Code 128".into(),
            ),
            Self::Phomemo40x30 => (
                "m110_40x30".into(),
                "Phomemo M110 40x30mm (1.57\"x1.18\")".into(),
                40.0,
                30.0,
                "Default Phomemo M110 / M120 / M220 starter roll".into(),
            ),
            Self::Phomemo50x30 => (
                "m110_50x30".into(),
                "Phomemo M110 50x30mm (1.97\"x1.18\")".into(),
                50.0,
                30.0,
                "Wide ~2-inch Phomemo M110 / M200 barcode label".into(),
            ),
            Self::Phomemo40x20 => (
                "m110_40x20".into(),
                "Phomemo M110 40x20mm (1.57\"x0.79\")".into(),
                40.0,
                20.0,
                "Compact Phomemo M110 organizer & drawer label".into(),
            ),
            Self::Phomemo30x20 => (
                "m110_30x20".into(),
                "Phomemo M110 30x20mm (1.18\"x0.79\")".into(),
                30.0,
                20.0,
                "Small Phomemo M110 label for SMD bags & micro drawers".into(),
            ),
            Self::Phomemo50x80 => (
                "m110_50x80".into(),
                "Phomemo M110/M200 50x80mm (1.97\"x3.15\")".into(),
                50.0,
                80.0,
                "Large Phomemo bin & storage box label".into(),
            ),
            Self::CustomMm10(w10, h10) => {
                let w = (*w10 as f64) / 10.0;
                let h = (*h10 as f64) / 10.0;
                (
                    format!("custom:{:.1}x{:.1}", w, h),
                    format!("Custom {:.1}mm x {:.1}mm ({:.2}\" x {:.2}\")", w, h, w / 25.4, h / 25.4),
                    w,
                    h,
                    "Custom thermal label size".into(),
                )
            }
        };
        let mm_to_pt = 72.0 / 25.4;
        LabelPresetInfo {
            id,
            name,
            width_mm: round2(w_mm),
            height_mm: round2(h_mm),
            width_pt: round2(w_mm * mm_to_pt),
            height_pt: round2(h_mm * mm_to_pt),
            description: desc,
        }
    }

    pub fn all_presets() -> Vec<LabelPresetInfo> {
        vec![
            Self::In2x1.info(),
            Self::In225x125.info(),
            Self::Ql62x29.info(),
            Self::Ql62x42.info(),
            Self::Phomemo40x30.info(),
            Self::Phomemo50x30.info(),
            Self::Phomemo40x20.info(),
            Self::Phomemo30x20.info(),
            Self::Phomemo50x80.info(),
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarcodeStyle {
    Code128,
    Qr,
    Combo,
}

impl BarcodeStyle {
    pub fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "code128" | "1d" => Self::Code128,
            "qr" | "2d" => Self::Qr,
            _ => Self::Combo,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TextRun {
    pub x: f64,
    pub y: f64,
    pub size: f64,
    pub bold: bool,
    pub mono: bool,
    pub text: String,
    pub right_align: bool,
}

#[derive(Debug, Clone)]
pub struct RectItem {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub fill: &'static str,
}

#[derive(Debug, Clone)]
pub struct LabelSpec {
    pub title: String,
    pub width_mm: f64,
    pub height_mm: f64,
    pub width_pt: f64,
    pub height_pt: f64,
    pub rects: Vec<RectItem>,
    pub texts: Vec<TextRun>,
}

pub fn build_part_label(
    part: &Part,
    preset: LabelPreset,
    style: BarcodeStyle,
) -> Result<LabelSpec> {
    let info = preset.info();
    let w = info.width_pt;
    let h = info.height_pt;
    let margin = if h < 65.0 || w < 100.0 { 3.5 } else { 5.0 };
    let mut rects = Vec::new();
    let mut texts = Vec::new();

    // Header bar
    let loc_label = part.location_code.as_deref().unwrap_or("UNBINNED");
    texts.push(TextRun {
        x: margin,
        y: margin + 9.0,
        size: 10.0,
        bold: true,
        mono: true,
        text: truncate_ascii(&part.sku, 16),
        right_align: false,
    });
    texts.push(TextRun {
        x: w - margin,
        y: margin + 8.5,
        size: 8.0,
        bold: true,
        mono: true,
        text: truncate_ascii(loc_label, 14),
        right_align: true,
    });
    // Divider line
    rects.push(RectItem {
        x: margin,
        y: margin + 11.5,
        w: w - margin * 2.0,
        h: 0.6,
        fill: "#000000",
    });

    // Encode payloads:
    // Use compact SKU text on Code 128 when space is tight (e.g. 2x1) so bar widths stay crisp,
    // and full `SKU:<sku>` in QR code.
    let qr_payload = if part.barcode_value.is_empty() {
        format!("SKU:{}", part.sku)
    } else {
        part.barcode_value.clone()
    };
    let c128_payload = part.sku.clone();

    let body_top = margin + 19.0;
    let sup_line = match (
        part.primary_supplier_name.as_deref(),
        part.mpn.as_str(),
        part.unit_cost > 0.0,
    ) {
        (Some(s), m, true) if !m.is_empty() => format!("{} | {} | ${:.2}", s, m, part.unit_cost),
        (Some(s), _, true) => format!("{} | ${:.2}", s, part.unit_cost),
        (_, m, true) if !m.is_empty() => format!("{} | ${:.2}", m, part.unit_cost),
        (_, _, true) => format!("Unit: ${:.2}", part.unit_cost),
        (Some(s), _, false) => s.to_string(),
        _ => String::new(),
    };

    match style {
        BarcodeStyle::Qr => {
            let qr_side = (h - body_top + 4.0).min(w * 0.40).max(18.0);
            let qr_x = w - margin - qr_side;
            let qr_y = margin + 14.0;
            let qr = encode_qr(&qr_payload)?;
            push_qr_rects(&mut rects, &qr, qr_x, qr_y, qr_side);

            let max_chars = ((qr_x - margin - 4.0) / 4.3) as usize;
            texts.push(TextRun {
                x: margin,
                y: body_top + 1.0,
                size: 8.5,
                bold: true,
                mono: false,
                text: truncate_ascii(&part.name, max_chars.max(12)),
                right_align: false,
            });
            texts.push(TextRun {
                x: margin,
                y: body_top + 11.0,
                size: 7.0,
                bold: false,
                mono: false,
                text: truncate_ascii(&part.spec, max_chars.max(14) + 3),
                right_align: false,
            });
            if !part.package.is_empty() {
                texts.push(TextRun {
                    x: margin,
                    y: body_top + 20.0,
                    size: 6.5,
                    bold: false,
                    mono: false,
                    text: truncate_ascii(&format!("Pkg: {}", part.package), max_chars.max(14) + 3),
                    right_align: false,
                });
            }
            if !sup_line.is_empty() && h >= 80.0 {
                texts.push(TextRun {
                    x: margin,
                    y: h - margin - 2.0,
                    size: 6.5,
                    bold: false,
                    mono: true,
                    text: truncate_ascii(&sup_line, max_chars.max(16) + 4),
                    right_align: false,
                });
            }
        }
        BarcodeStyle::Code128 => {
            let max_chars = ((w - margin * 2.0) / 4.2) as usize;
            texts.push(TextRun {
                x: margin,
                y: body_top,
                size: 8.0,
                bold: true,
                mono: false,
                text: truncate_ascii(&part.name, max_chars.max(18)),
                right_align: false,
            });
            let sub = if part.package.is_empty() {
                part.spec.clone()
            } else {
                format!("{} ({})", part.spec, part.package)
            };
            texts.push(TextRun {
                x: margin,
                y: body_top + 9.0,
                size: 6.5,
                bold: false,
                mono: false,
                text: truncate_ascii(&sub, max_chars.max(22) + 4),
                right_align: false,
            });

            let c128 = encode_code128b(&c128_payload)?;
            let bar_y = body_top + 13.0;
            let bar_h = (h - bar_y - margin - 8.0).max(16.0);
            push_c128_rects(&mut rects, &c128, margin, bar_y, w - margin * 2.0, bar_h);
            texts.push(TextRun {
                x: w / 2.0,
                y: h - margin - 1.0,
                size: 6.5,
                bold: false,
                mono: true,
                text: c128_payload,
                right_align: false,
            });
        }
        BarcodeStyle::Combo => {
            let qr_side = ((h - (margin + 14.0) - margin) * 0.62).clamp(18.0, 44.0);
            let qr_x = w - margin - qr_side;
            let qr_y = margin + 13.5;
            let qr = encode_qr(&qr_payload)?;
            push_qr_rects(&mut rects, &qr, qr_x, qr_y, qr_side);

            let text_w = qr_x - margin - 4.0;
            let max_chars = (text_w / 4.1) as usize;
            texts.push(TextRun {
                x: margin,
                y: body_top,
                size: 8.0,
                bold: true,
                mono: false,
                text: truncate_ascii(&part.name, max_chars.max(12)),
                right_align: false,
            });
            texts.push(TextRun {
                x: margin,
                y: body_top + 9.0,
                size: 6.5,
                bold: false,
                mono: false,
                text: truncate_ascii(&part.spec, max_chars.max(14) + 3),
                right_align: false,
            });
            if !sup_line.is_empty() && h >= 78.0 {
                texts.push(TextRun {
                    x: margin,
                    y: body_top + 18.0,
                    size: 6.0,
                    bold: false,
                    mono: false,
                    text: truncate_ascii(&sup_line, max_chars.max(16) + 4),
                    right_align: false,
                });
            }

            let c128 = encode_code128b(&c128_payload)?;
            let bar_y = (qr_y + qr_side + 2.0).min(h - margin - 20.0);
            let bar_h = (h - bar_y - margin - 7.5).max(12.0);
            push_c128_rects(&mut rects, &c128, margin, bar_y, w - margin * 2.0, bar_h);
            texts.push(TextRun {
                x: margin,
                y: h - margin - 0.5,
                size: 6.0,
                bold: false,
                mono: true,
                text: truncate_ascii(&c128_payload, 24),
                right_align: false,
            });
        }
    }

    Ok(LabelSpec {
        title: format!("Part Label: {}", part.sku),
        width_mm: info.width_mm,
        height_mm: info.height_mm,
        width_pt: w,
        height_pt: h,
        rects,
        texts,
    })
}

pub fn build_bin_label(
    loc: &LocationWithStats,
    parts_in_bin: &[Part],
    preset: LabelPreset,
    style: BarcodeStyle,
) -> Result<LabelSpec> {
    let info = preset.info();
    let w = info.width_pt;
    let h = info.height_pt;
    let margin = if h < 65.0 || w < 100.0 { 3.5 } else { 5.0 };
    let mut rects = Vec::new();
    let mut texts = Vec::new();

    // Header: Large Bin Code + Area/Rack
    texts.push(TextRun {
        x: margin,
        y: margin + 10.5,
        size: 12.0,
        bold: true,
        mono: true,
        text: truncate_ascii(&loc.location.code, 16),
        right_align: false,
    });
    let zone = [
        loc.location.area.as_str(),
        loc.location.rack.as_str(),
        loc.location.drawer.as_str(),
    ]
    .into_iter()
    .filter(|s| !s.is_empty())
    .collect::<Vec<_>>()
    .join("/");
    if !zone.is_empty() {
        texts.push(TextRun {
            x: w - margin,
            y: margin + 9.5,
            size: 7.0,
            bold: false,
            mono: false,
            text: truncate_ascii(&zone, 20),
            right_align: true,
        });
    }
    rects.push(RectItem {
        x: margin,
        y: margin + 13.5,
        w: w - margin * 2.0,
        h: 0.7,
        fill: "#000000",
    });

    let qr_payload = if loc.location.barcode_value.is_empty() {
        format!("BIN:{}", loc.location.code)
    } else {
        loc.location.barcode_value.clone()
    };
    let c128_payload = loc.location.code.clone();

    let content_top = margin + 21.0;

    match style {
        BarcodeStyle::Code128 => {
            let max_lines = if h >= 100.0 { 4 } else { 2 };
            let max_chars = ((w - margin * 2.0) / 4.0) as usize;
            for (idx, p) in parts_in_bin.iter().take(max_lines).enumerate() {
                texts.push(TextRun {
                    x: margin,
                    y: content_top + (idx as f64) * 8.5,
                    size: 6.5,
                    bold: false,
                    mono: true,
                    text: truncate_ascii(&format!("{} {}", p.sku, p.name), max_chars),
                    right_align: false,
                });
            }
            if parts_in_bin.is_empty() {
                texts.push(TextRun {
                    x: margin,
                    y: content_top,
                    size: 7.0,
                    bold: false,
                    mono: false,
                    text: truncate_ascii(&loc.location.name, max_chars),
                    right_align: false,
                });
            }
            let c128 = encode_code128b(&c128_payload)?;
            let bar_y = h - margin - 28.0;
            push_c128_rects(&mut rects, &c128, margin, bar_y, w - margin * 2.0, 20.0);
            texts.push(TextRun {
                x: margin,
                y: h - margin - 1.0,
                size: 6.5,
                bold: false,
                mono: true,
                text: qr_payload,
                right_align: false,
            });
        }
        BarcodeStyle::Qr => {
            let qr_side = (h - content_top + 4.0).min(w * 0.42).max(18.0);
            let qr_x = w - margin - qr_side;
            let qr_y = margin + 15.5;
            let qr = encode_qr(&qr_payload)?;
            push_qr_rects(&mut rects, &qr, qr_x, qr_y, qr_side);

            let max_chars = ((qr_x - margin - 4.0) / 3.9) as usize;
            let max_lines = ((h - content_top - margin) / 8.5).floor() as usize;
            for (idx, p) in parts_in_bin.iter().take(max_lines.max(1)).enumerate() {
                texts.push(TextRun {
                    x: margin,
                    y: content_top + (idx as f64) * 8.5,
                    size: 6.5,
                    bold: false,
                    mono: true,
                    text: truncate_ascii(&format!("{} {}", p.sku, p.name), max_chars.max(12)),
                    right_align: false,
                });
            }
            if parts_in_bin.len() > max_lines.max(1) {
                texts.push(TextRun {
                    x: margin,
                    y: h - margin - 1.0,
                    size: 6.0,
                    bold: true,
                    mono: false,
                    text: format!("+{} more parts (${:.2})", parts_in_bin.len() - max_lines.max(1), loc.total_value),
                    right_align: false,
                });
            }
        }
        BarcodeStyle::Combo => {
            let qr_side = ((h - (margin + 15.0) - margin) * 0.56).clamp(18.0, 48.0);
            let qr_x = w - margin - qr_side;
            let qr_y = margin + 15.5;
            let qr = encode_qr(&qr_payload)?;
            push_qr_rects(&mut rects, &qr, qr_x, qr_y, qr_side);

            let text_w = qr_x - margin - 4.0;
            let max_chars = (text_w / 3.9) as usize;
            let avail_for_list = (qr_side - 2.0).max(16.0);
            let max_lines = (avail_for_list / 8.2).floor() as usize;

            if parts_in_bin.is_empty() {
                texts.push(TextRun {
                    x: margin,
                    y: content_top,
                    size: 7.0,
                    bold: false,
                    mono: false,
                    text: truncate_ascii(&loc.location.name, max_chars.max(12)),
                    right_align: false,
                });
            } else {
                for (idx, p) in parts_in_bin.iter().take(max_lines.max(1)).enumerate() {
                    texts.push(TextRun {
                        x: margin,
                        y: content_top + (idx as f64) * 8.2,
                        size: 6.2,
                        bold: false,
                        mono: true,
                        text: truncate_ascii(&format!("{} {}", p.sku, p.name), max_chars.max(12)),
                        right_align: false,
                    });
                }
            }

            let c128 = encode_code128b(&c128_payload)?;
            let bar_y = (qr_y + qr_side + 2.5).min(h - margin - 20.0);
            let bar_h = (h - bar_y - margin - 7.5).max(12.0);
            push_c128_rects(&mut rects, &c128, margin, bar_y, w - margin * 2.0, bar_h);
            texts.push(TextRun {
                x: margin,
                y: h - margin - 0.5,
                size: 6.0,
                bold: false,
                mono: true,
                text: truncate_ascii(&format!("{} | {} SKUs", qr_payload, loc.part_count), 32),
                right_align: false,
            });
        }
    }

    Ok(LabelSpec {
        title: format!("Bin Label: {}", loc.location.code),
        width_mm: info.width_mm,
        height_mm: info.height_mm,
        width_pt: w,
        height_pt: h,
        rects,
        texts,
    })
}

pub fn render_label_svg(spec: &LabelSpec) -> String {
    let mut out = String::with_capacity(4096);
    out.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{:.2}mm\" height=\"{:.2}mm\" viewBox=\"0 0 {:.2} {:.2}\" shape-rendering=\"crispEdges\">\n",
        spec.width_mm, spec.height_mm, spec.width_pt, spec.height_pt
    ));
    out.push_str("<rect x=\"0\" y=\"0\" width=\"100%\" height=\"100%\" fill=\"#ffffff\"/>\n");
    for r in &spec.rects {
        out.push_str(&format!(
            "<rect x=\"{:.3}\" y=\"{:.3}\" width=\"{:.3}\" height=\"{:.3}\" fill=\"{}\"/>\n",
            r.x, r.y, r.w, r.h, r.fill
        ));
    }
    for t in &spec.texts {
        let family = if t.mono {
            "monospace"
        } else {
            "sans-serif"
        };
        let weight = if t.bold { "bold" } else { "normal" };
        let anchor = if t.right_align { "end" } else { "start" };
        out.push_str(&format!(
            "<text x=\"{:.2}\" y=\"{:.2}\" font-family=\"{}\" font-size=\"{:.2}\" font-weight=\"{}\" text-anchor=\"{}\" fill=\"#000000\">{}</text>\n",
            t.x,
            t.y,
            family,
            t.size,
            weight,
            anchor,
            xml_escape(&t.text)
        ));
    }
    out.push_str("</svg>\n");
    out
}

pub fn render_printable_html(spec: &LabelSpec) -> String {
    let svg = render_label_svg(spec);
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{title}</title>\
        <style>@page{{size:{w:.2}mm {h:.2}mm;margin:0}}body{{margin:0;padding:0;background:#fff;display:flex;align-items:center;justify-content:center}}svg{{display:block}}</style>\
        </head><body>{svg}<script>window.onload=()=>setTimeout(()=>window.print(),150);</script></body></html>",
        title = xml_escape(&spec.title),
        w = spec.width_mm,
        h = spec.height_mm,
        svg = svg
    )
}

fn push_qr_rects(rects: &mut Vec<RectItem>, qr: &QrMatrix, x: f64, y: f64, side: f64) {
    // White quiet backing
    rects.push(RectItem {
        x,
        y,
        w: side,
        h: side,
        fill: "#ffffff",
    });
    let n = qr.size_modules;
    let mod_sz = side / (n as f64);
    let mut grid = vec![false; n * n];
    for &(col, row) in &qr.dark_modules {
        if row < n && col < n {
            grid[row * n + col] = true;
        }
    }
    // Merge horizontal dark runs per row and add slight vertical overlap to prevent subpixel SVG seams
    for row in 0..n {
        let mut col = 0;
        while col < n {
            if !grid[row * n + col] {
                col += 1;
                continue;
            }
            let start = col;
            while col < n && grid[row * n + col] {
                col += 1;
            }
            let run_w = ((col - start) as f64) * mod_sz;
            rects.push(RectItem {
                x: x + (start as f64) * mod_sz,
                y: y + (row as f64) * mod_sz,
                w: run_w + 0.015,
                h: mod_sz + 0.02,
                fill: "#000000",
            });
        }
    }
}

fn push_c128_rects(
    rects: &mut Vec<RectItem>,
    c128: &Code128Encoded,
    x: f64,
    y: f64,
    max_w: f64,
    bar_h: f64,
) {
    let mod_w = max_w / (c128.total_modules as f64);
    rects.push(RectItem {
        x,
        y,
        w: max_w,
        h: bar_h,
        fill: "#ffffff",
    });
    for bar in &c128.bars {
        rects.push(RectItem {
            x: x + (bar.x_mod as f64) * mod_w,
            y,
            w: (bar.w_mod as f64) * mod_w,
            h: bar_h,
            fill: "#000000",
        });
    }
}

fn truncate_ascii(s: &str, max_len: usize) -> String {
    let ascii = to_clean_ascii(s);
    if ascii.len() <= max_len {
        ascii
    } else if max_len > 3 {
        format!("{}...", &ascii[..max_len - 3])
    } else {
        ascii[..max_len].to_string()
    }
}

pub fn to_clean_ascii(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\u{00D7}' => out.push('x'),
            '\u{2014}' | '\u{2013}' => out.push('-'),
            '\u{2192}' => out.push_str("->"),
            c if c.is_ascii() && !c.is_ascii_control() => out.push(c),
            _ => out.push(' '),
        }
    }
    out.trim().to_string()
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}
