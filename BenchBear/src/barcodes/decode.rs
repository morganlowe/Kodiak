use anyhow::{anyhow, Result};
use std::fs;
use std::process::Command;

use crate::db::Database;
use crate::models::DecodedBarcode;

pub fn zbar_available() -> bool {
    Command::new("zbarimg")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn decode_image_bytes(bytes: &[u8], db: &Database) -> Result<Vec<DecodedBarcode>> {
    // Normalize input into PNG bytes so zbarimg can read WebP/JPEG/BMP/PNG uniformly
    let img = image::load_from_memory(bytes)
        .map_err(|e| anyhow!("Unsupported or invalid image data: {}", e))?;

    let tmp_path = std::env::temp_dir().join(format!(
        "parts_wiki_scan_{}_{}.png",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    ));

    img.save_with_format(&tmp_path, image::ImageFormat::Png)?;

    let mut raw_hits: Vec<(String, String, String)> = Vec::new(); // (format, payload, engine)

    if let Ok(output) = Command::new("zbarimg")
        .args(["--quiet", "-S*.enable"])
        .arg(&tmp_path)
        .output()
    {
        if output.status.success() || !output.stdout.is_empty() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                let l = line.trim();
                if l.is_empty() {
                    continue;
                }
                if let Some((fmt, payload)) = l.split_once(':') {
                    raw_hits.push((fmt.to_string(), payload.to_string(), "zbarimg".to_string()));
                } else {
                    raw_hits.push(("UNKNOWN".to_string(), l.to_string(), "zbarimg".to_string()));
                }
            }
        }
    }
    let _ = fs::remove_file(&tmp_path);

    // Pure-Rust QR fallback via rqrr if zbarimg wasn't found or missed a QR code
    if raw_hits.is_empty() {
        let luma = img.to_luma8();
        let mut prepared = rqrr::PreparedImage::prepare(luma);
        for grid in prepared.detect_grids() {
            if let Ok((_meta, content)) = grid.decode() {
                if !content.is_empty() {
                    raw_hits.push(("QR-Code".to_string(), content, "rqrr".to_string()));
                }
            }
        }
    }

    let mut results = Vec::with_capacity(raw_hits.len());
    for (format, raw_text, engine) in raw_hits {
        let resolved = db.resolve_barcode(&raw_text)?;
        results.push(DecodedBarcode {
            format,
            raw_text,
            engine,
            resolved,
        });
    }
    Ok(results)
}
