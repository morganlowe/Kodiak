use anyhow::{anyhow, Result};
use std::fmt::Write as _;

use crate::barcodes::labels::{to_clean_ascii, LabelSpec};

/// Renders one or more LabelSpecs into a vector PDF 1.4 document with exact page sizes in points.
pub fn render_labels_pdf(labels: &[LabelSpec]) -> Result<Vec<u8>> {
    if labels.is_empty() {
        return Err(anyhow!("Cannot generate PDF with zero labels"));
    }

    // Object numbering:
    // 1: Catalog
    // 2: Pages
    // 3: Font F1 (Helvetica)
    // 4: Font F2 (Helvetica-Bold)
    // 5: Font F3 (Courier)
    // 6: Font F4 (Courier-Bold)
    // For each label i (0..N):
    //   page_obj_id = 7 + i * 2
    //   stream_obj_id = 7 + i * 2 + 1
    let mut objects: Vec<String> = Vec::new();
    let page_ids: Vec<usize> = (0..labels.len()).map(|i| 7 + i * 2).collect();
    let kids_str = page_ids
        .iter()
        .map(|id| format!("{} 0 R", id))
        .collect::<Vec<_>>()
        .join(" ");

    objects.push("<< /Type /Catalog /Pages 2 0 R >>".to_string());
    objects.push(format!(
        "<< /Type /Pages /Kids [{}] /Count {} >>",
        kids_str,
        labels.len()
    ));
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string());
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold >>".to_string());
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".to_string());
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Courier-Bold >>".to_string());

    for (i, spec) in labels.iter().enumerate() {
        let stream_id = 7 + i * 2 + 1;
        let stream = build_page_stream(spec);
        let page_obj = format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {:.2} {:.2}] /Resources << /Font << /F1 3 0 R /F2 4 0 R /F3 5 0 R /F4 6 0 R >> >> /Contents {} 0 R >>",
            spec.width_pt, spec.height_pt, stream_id
        );
        let stream_obj = format!(
            "<< /Length {} >>\nstream\n{}endstream",
            stream.len(),
            stream
        );
        objects.push(page_obj);
        objects.push(stream_obj);
    }

    let mut pdf = Vec::with_capacity(8192);
    pdf.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");

    let mut offsets = Vec::with_capacity(objects.len() + 1);
    offsets.push(0usize);

    for (idx, obj_body) in objects.iter().enumerate() {
        let obj_num = idx + 1;
        offsets.push(pdf.len());
        let entry = format!("{} 0 obj\n{}\nendobj\n", obj_num, obj_body);
        pdf.extend_from_slice(entry.as_bytes());
    }

    let xref_offset = pdf.len();
    let mut xref = format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1);
    for off in offsets.iter().skip(1) {
        let _ = writeln!(xref, "{:010} 00000 n ", off);
    }
    let _ = write!(
        xref,
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
        objects.len() + 1,
        xref_offset
    );
    pdf.extend_from_slice(xref.as_bytes());
    Ok(pdf)
}

fn build_page_stream(spec: &LabelSpec) -> String {
    let mut s = String::with_capacity(4096);
    let h = spec.height_pt;

    // Explicit white page background
    let _ = writeln!(s, "1 1 1 rg 0 0 {:.2} {:.2} re f", spec.width_pt, h);

    for r in &spec.rects {
        let pdf_y = h - r.y - r.h;
        if r.fill == "#ffffff" {
            let _ = writeln!(s, "1 1 1 rg {:.3} {:.3} {:.3} {:.3} re f", r.x, pdf_y, r.w, r.h);
        } else {
            let _ = writeln!(s, "0 0 0 rg {:.3} {:.3} {:.3} {:.3} re f", r.x, pdf_y, r.w, r.h);
        }
    }

    let _ = writeln!(s, "0 0 0 rg");
    for t in &spec.texts {
        let clean = to_clean_ascii(&t.text);
        if clean.is_empty() {
            continue;
        }
        let font = match (t.mono, t.bold) {
            (false, false) => "F1",
            (false, true) => "F2",
            (true, false) => "F3",
            (true, true) => "F4",
        };
        let pdf_y = h - t.y;
        let x = if t.right_align {
            let est_w = estimate_text_width(&clean, t.size, t.mono);
            (t.x - est_w).max(2.0)
        } else {
            t.x
        };
        let _ = writeln!(
            s,
            "BT /{} {:.2} Tf 1 0 0 1 {:.2} {:.2} Tm ({}) Tj ET",
            font,
            t.size,
            x,
            pdf_y,
            pdf_escape(&clean)
        );
    }
    s
}

fn estimate_text_width(text: &str, size: f64, mono: bool) -> f64 {
    let factor = if mono { 0.60 } else { 0.52 };
    text.len() as f64 * size * factor
}

fn pdf_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::barcodes::labels::{build_part_label, BarcodeStyle, LabelPreset};
    use crate::models::Part;

    #[test]
    fn generates_valid_pdf_header_and_eof() {
        let part = Part {
            id: 1,
            sku: "MCU-001".to_string(),
            category: "microcontrollers".to_string(),
            name: "ESP32-S3-WROOM-1".to_string(),
            spec: "Dual-core 240MHz".to_string(),
            package: "SMD Module".to_string(),
            qty: 12,
            min_qty: 3,
            unit_cost: 4.25,
            currency: "USD".to_string(),
            location_id: Some(1),
            location_code: Some("BIN-A1".to_string()),
            location_name: Some("Bin A1".to_string()),
            primary_supplier_id: Some(1),
            primary_supplier_name: Some("DigiKey".to_string()),
            mpn: "ESP32-S3-WROOM-1-N8R8".to_string(),
            supplier_sku: "1965-ESP32".to_string(),
            product_url: String::new(),
            datasheet_url: String::new(),
            barcode_value: "SKU:MCU-001".to_string(),
            projects_tags: "rover".to_string(),
            description: String::new(),
            notes: String::new(),
            image_path: String::new(),
            pinout_image_path: String::new(),
            created_at: String::new(),
            updated_at: String::new(),
            extended_value: 51.0,
            low_stock: false,
        };
        let spec = build_part_label(&part, LabelPreset::In2x1, BarcodeStyle::Combo).unwrap();
        let pdf = render_labels_pdf(&[spec]).unwrap();
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.starts_with("%PDF-1.4"));
        assert!(text.contains("/MediaBox [0 0 144.00 72.00]"));
        assert!(text.trim_end().ends_with("%%EOF"));
    }
}
