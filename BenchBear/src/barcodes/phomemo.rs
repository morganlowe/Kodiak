use anyhow::{anyhow, Result};
use image::{imageops::FilterType, DynamicImage, GrayImage, ImageFormat, Luma};
use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use crate::barcodes::labels::LabelSpec;

pub const PHOMEMO_DPI: f64 = 203.0;
pub const M110_MAX_WIDTH_DOTS: u32 = 384;

#[derive(Debug, Clone, Serialize)]
pub struct ThermalDeviceInfo {
    pub path: String,
    pub transport: String,
    pub writable: bool,
}

pub fn validate_thermal_device_path(raw: &str) -> Result<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("Device path cannot be empty"));
    }
    let linux_prefixes = [
        "/dev/usb/lp",
        "/dev/rfcomm",
        "/dev/ttyACM",
        "/dev/ttyUSB",
    ];
    for prefix in linux_prefixes {
        if let Some(suffix) = trimmed.strip_prefix(prefix) {
            if !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()) {
                return Ok(PathBuf::from(trimmed));
            }
        }
    }
    // macOS serial / Bluetooth SPP callout or tty devices
    for prefix in ["/dev/cu.", "/dev/tty."] {
        if let Some(suffix) = trimmed.strip_prefix(prefix) {
            if !suffix.is_empty()
                && suffix.len() <= 64
                && !suffix.contains("..")
                && suffix
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
            {
                return Ok(PathBuf::from(trimmed));
            }
        }
    }
    // Windows COM ports: COM1..=COM256 or \\.\COM1..=\\.\COM256
    let upper = trimmed.to_ascii_uppercase();
    let com_digits = upper
        .strip_prefix(r"\\.\COM")
        .or_else(|| upper.strip_prefix("COM"));
    if let Some(digits) = com_digits {
        if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(port_num) = digits.parse::<u16>() {
                if (1..=256).contains(&port_num) {
                    return Ok(PathBuf::from(format!(r"\\.\COM{}", port_num)));
                }
            }
        }
    }
    Err(anyhow!(
        "Device path '{}' is not allowed. Use /dev/usb/lp0 or /dev/rfcomm0 (Linux), /dev/cu.<name> (macOS), or COM3 / \\\\.\\COM3 (Windows)",
        trimmed
    ))
}

pub fn list_thermal_devices() -> Vec<ThermalDeviceInfo> {
    let mut devices = Vec::new();
    let scan_dir = |dir: &str, prefix: &str, transport: &str, out: &mut Vec<ThermalDeviceInfo>| {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    if let Some(suffix) = name.strip_prefix(prefix) {
                        if !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()) {
                            let p = entry.path();
                            let writable = OpenOptions::new().write(true).open(&p).is_ok();
                            out.push(ThermalDeviceInfo {
                                path: p.to_string_lossy().into_owned(),
                                transport: transport.to_string(),
                                writable,
                            });
                        }
                    }
                }
            }
        }
    };

    #[cfg(target_os = "linux")]
    {
        scan_dir("/dev/usb", "lp", "USB Printer Class", &mut devices);
        scan_dir("/dev", "rfcomm", "Bluetooth SPP (/dev/rfcomm*)", &mut devices);
        scan_dir("/dev", "ttyACM", "USB Serial ACM", &mut devices);
        scan_dir("/dev", "ttyUSB", "USB Serial", &mut devices);
    }

    #[cfg(target_os = "macos")]
    {
        let _ = scan_dir;
        if let Ok(entries) = fs::read_dir("/dev") {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    if name.starts_with("cu.") {
                        let lower = name.to_ascii_lowercase();
                        if ["usbmodem", "usbserial", "phomemo", "m110", "m120", "m200", "m220"]
                            .iter()
                            .any(|k| lower.contains(k))
                        {
                            let p = entry.path();
                            let writable = OpenOptions::new().write(true).open(&p).is_ok();
                            devices.push(ThermalDeviceInfo {
                                path: p.to_string_lossy().into_owned(),
                                transport: "macOS Serial / Bluetooth (/dev/cu.*)".to_string(),
                                writable,
                            });
                        }
                    }
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        let _ = scan_dir;
        for port in 1..=32u16 {
            let win_path = format!(r"\\.\COM{}", port);
            if let Ok(_) = OpenOptions::new().write(true).open(&win_path) {
                devices.push(ThermalDeviceInfo {
                    path: format!("COM{}", port),
                    transport: "Windows COM Port (USB/Bluetooth)".to_string(),
                    writable: true,
                });
            }
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        scan_dir("/dev/usb", "lp", "USB Printer Class", &mut devices);
    }

    devices.sort_by(|a, b| a.path.cmp(&b.path));
    devices
}

pub fn decode_image_bytes_to_luma(bytes: &[u8]) -> Result<GrayImage> {
    let dyn_img = image::load_from_memory(bytes)
        .map_err(|e| anyhow!("Failed to decode raster label image: {}", e))?;
    Ok(dyn_img.to_luma8())
}

pub fn rasterize_label_spec_to_luma(spec: &LabelSpec, dpi: f64) -> GrayImage {
    let scale = dpi / 72.0;
    let width = ((spec.width_pt * scale).round() as u32).max(32);
    let height = ((spec.height_pt * scale).round() as u32).max(24);
    let mut img = GrayImage::from_pixel(width, height, Luma([255u8]));

    for r in &spec.rects {
        let color = if r.fill.eq_ignore_ascii_case("#ffffff") || r.fill.eq_ignore_ascii_case("white") {
            255u8
        } else {
            0u8
        };
        let x0 = (r.x * scale).round().max(0.0) as u32;
        let y0 = (r.y * scale).round().max(0.0) as u32;
        let x1 = ((r.x + r.w) * scale).round().min(width as f64) as u32;
        let y1 = ((r.y + r.h) * scale).round().min(height as f64) as u32;
        for py in y0..y1 {
            for px in x0..x1 {
                img.put_pixel(px, py, Luma([color]));
            }
        }
    }

    for t in &spec.texts {
        let char_scale = ((t.size * scale * 0.72) / 7.0).round().max(1.0) as u32;
        let advance = 6 * char_scale;
        let text_w = (t.text.chars().count() as u32) * advance;
        let base_x = (t.x * scale).round() as i32;
        let start_x = if t.right_align {
            base_x - (text_w as i32)
        } else {
            base_x
        };
        let start_y = (t.y * scale).round() as i32 - (7 * char_scale) as i32;

        for (ci, ch) in t.text.chars().enumerate() {
            let glyph = glyph_5x7(ch);
            let gx0 = start_x + (ci as i32) * (advance as i32);
            for (row_i, row_bits) in glyph.iter().enumerate() {
                for col_i in 0..5 {
                    if (row_bits >> (4 - col_i)) & 1 == 1 {
                        for sy in 0..char_scale {
                            for sx in 0..char_scale {
                                let px = gx0 + (col_i as i32) * (char_scale as i32) + sx as i32;
                                let py = start_y + (row_i as i32) * (char_scale as i32) + sy as i32;
                                if px >= 0 && py >= 0 && (px as u32) < width && (py as u32) < height {
                                    img.put_pixel(px as u32, py as u32, Luma([0u8]));
                                    if t.bold && ((px + 1) as u32) < width {
                                        img.put_pixel((px + 1) as u32, py as u32, Luma([0u8]));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    img
}

pub fn render_label_png(spec: &LabelSpec, dpi: f64) -> Result<Vec<u8>> {
    let gray = rasterize_label_spec_to_luma(spec, dpi);
    let mut buf = Cursor::new(Vec::new());
    DynamicImage::ImageLuma8(gray)
        .write_to(&mut buf, ImageFormat::Png)
        .map_err(|e| anyhow!("PNG encode failed: {}", e))?;
    Ok(buf.into_inner())
}

pub fn encode_m110_packet(
    gray: &GrayImage,
    density: u8,
    media_type: &str,
    copies: usize,
) -> Vec<u8> {
    let fitted = if gray.width() > M110_MAX_WIDTH_DOTS {
        let new_h = ((gray.height() as f64) * (M110_MAX_WIDTH_DOTS as f64) / (gray.width() as f64))
            .round()
            .max(1.0) as u32;
        image::imageops::resize(gray, M110_MAX_WIDTH_DOTS, new_h, FilterType::Nearest)
    } else {
        gray.clone()
    };

    let w = fitted.width().max(1);
    let h = fitted.height().max(1);
    let bytes_per_line = (w + 7) / 8;

    let mut bitmap = Vec::with_capacity((bytes_per_line * h) as usize);
    for y in 0..h {
        for bx in 0..bytes_per_line {
            let mut byte = 0u8;
            for bit in 0..8 {
                let x = bx * 8 + bit;
                if x < w && fitted.get_pixel(x, y).0[0] < 160 {
                    byte |= 1u8 << (7 - bit);
                }
            }
            bitmap.push(byte);
        }
    }

    let media_byte: u8 = match media_type.trim().to_ascii_lowercase().as_str() {
        "continuous" | "cont" => 0x0B,
        "black_mark" | "mark" => 0x26,
        _ => 0x0A, // "gap" die-cut labels
    };
    let d_val = density.clamp(1, 15);
    let copies = copies.clamp(1, 50);

    let x_l = (bytes_per_line & 0xFF) as u8;
    let x_h = ((bytes_per_line >> 8) & 0xFF) as u8;
    let y_l = (h & 0xFF) as u8;
    let y_h = ((h >> 8) & 0xFF) as u8;

    let mut packet = Vec::with_capacity((bitmap.len() + 32) * copies);
    for _ in 0..copies {
        // Print speed
        packet.extend_from_slice(&[0x1B, 0x4E, 0x0D, 0x05]);
        // Print darkness / density
        packet.extend_from_slice(&[0x1B, 0x4E, 0x04, d_val]);
        // Media type (gap / continuous / black mark)
        packet.extend_from_slice(&[0x1F, 0x11, media_byte]);
        // GS v 0 monochrome raster bitmap header + rows
        packet.extend_from_slice(&[0x1D, 0x76, 0x30, 0x00, x_l, x_h, y_l, y_h]);
        packet.extend_from_slice(&bitmap);
        // Feed / align to next label gap
        packet.extend_from_slice(&[0x1F, 0xF0, 0x05, 0x00, 0x1F, 0xF0, 0x03, 0x00]);
    }
    packet
}

pub fn write_packet_to_device(device_path: &str, packet: &[u8]) -> Result<usize> {
    let valid_path = validate_thermal_device_path(device_path)?;
    if !Path::new(&valid_path).exists() {
        return Err(anyhow!(
            "Printer device {} does not exist. Check USB cable or Bluetooth rfcomm bind.",
            valid_path.display()
        ));
    }
    let mut file = OpenOptions::new()
        .write(true)
        .open(&valid_path)
        .map_err(|e| anyhow!("Cannot open {} for writing: {}", valid_path.display(), e))?;
    file.write_all(packet)?;
    file.flush()?;
    Ok(packet.len())
}

fn glyph_5x7(ch: char) -> [u8; 7] {
    match ch.to_ascii_uppercase() {
        '0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        '1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        '2' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
        '3' => [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E],
        '4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        '5' => [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
        '6' => [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        '9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
        'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        'C' => [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
        'D' => [0x1C, 0x12, 0x11, 0x11, 0x11, 0x12, 0x1C],
        'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        'G' => [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0F],
        'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'I' => [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
        'J' => [0x07, 0x02, 0x02, 0x02, 0x02, 0x12, 0x0C],
        'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        'N' => [0x11, 0x11, 0x19, 0x15, 0x13, 0x11, 0x11],
        'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        'Q' => [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D],
        'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
        'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0A],
        'X' => [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11],
        'Y' => [0x11, 0x11, 0x11, 0x0A, 0x04, 0x04, 0x04],
        'Z' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F],
        '-' => [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00],
        '_' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1F],
        '.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C],
        ',' => [0x00, 0x00, 0x00, 0x00, 0x04, 0x04, 0x08],
        ':' => [0x00, 0x0C, 0x0C, 0x00, 0x0C, 0x0C, 0x00],
        '/' => [0x01, 0x02, 0x02, 0x04, 0x08, 0x08, 0x10],
        '|' => [0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        '+' => [0x00, 0x04, 0x04, 0x1F, 0x04, 0x04, 0x00],
        '$' => [0x04, 0x0F, 0x14, 0x0E, 0x05, 0x1E, 0x04],
        '(' => [0x02, 0x04, 0x08, 0x08, 0x08, 0x04, 0x02],
        ')' => [0x08, 0x04, 0x02, 0x02, 0x02, 0x04, 0x08],
        '#' => [0x0A, 0x0A, 0x1F, 0x0A, 0x1F, 0x0A, 0x0A],
        '%' => [0x19, 0x1A, 0x02, 0x04, 0x08, 0x0B, 0x13],
        '&' => [0x0C, 0x12, 0x14, 0x08, 0x15, 0x12, 0x0D],
        '=' => [0x00, 0x00, 0x1F, 0x00, 0x1F, 0x00, 0x00],
        '<' => [0x02, 0x04, 0x08, 0x10, 0x08, 0x04, 0x02],
        '>' => [0x08, 0x04, 0x02, 0x01, 0x02, 0x04, 0x08],
        ' ' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        _ => [0x00, 0x0E, 0x11, 0x11, 0x11, 0x0E, 0x00],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::barcodes::labels::{build_part_label, BarcodeStyle, LabelPreset};
    use crate::models::Part;

    #[test]
    fn validates_device_paths_strictly() {
        assert!(validate_thermal_device_path("/dev/usb/lp0").is_ok());
        assert!(validate_thermal_device_path("/dev/rfcomm0").is_ok());
        assert!(validate_thermal_device_path("/dev/ttyACM1").is_ok());
        assert!(validate_thermal_device_path("/dev/ttyUSB2").is_ok());
        assert!(validate_thermal_device_path("/dev/cu.PhomemoM110").is_ok());
        assert!(validate_thermal_device_path("/dev/cu.usbmodem14101").is_ok());
        assert_eq!(
            validate_thermal_device_path("COM3").unwrap(),
            PathBuf::from(r"\\.\COM3")
        );
        assert_eq!(
            validate_thermal_device_path(r"\\.\com12").unwrap(),
            PathBuf::from(r"\\.\COM12")
        );

        assert!(validate_thermal_device_path("").is_err());
        assert!(validate_thermal_device_path("/tmp/evil").is_err());
        assert!(validate_thermal_device_path("/dev/sda1").is_err());
        assert!(validate_thermal_device_path("/dev/usb/lp0/../foo").is_err());
        assert!(validate_thermal_device_path("/dev/cu.foo/../bar").is_err());
        assert!(validate_thermal_device_path("/dev/usb/lp").is_err());
        assert!(validate_thermal_device_path("COM9999").is_err());
    }

    #[test]
    fn custom_mm_and_inch_label_dimensions() {
        let p_mm = LabelPreset::from_dimensions(None, Some(40.0), Some(30.0), None, None);
        assert_eq!(p_mm.info().width_mm, 40.0);
        assert_eq!(p_mm.info().height_mm, 30.0);

        let p_in = LabelPreset::from_dimensions(None, None, None, Some(2.0), Some(1.0));
        assert_eq!(p_in.info().width_mm, 50.8);
        assert_eq!(p_in.info().height_mm, 25.4);

        let p_m110 = LabelPreset::from_str("m110_40x30");
        assert_eq!(p_m110.info().id, "m110_40x30");
        assert_eq!(p_m110.info().width_mm, 40.0);
    }

    #[test]
    fn encodes_valid_m110_escpos_packet_and_png() {
        let part = Part {
            id: 1,
            sku: "MCU-001".to_string(),
            category: "microcontrollers".to_string(),
            name: "ESP32-S3 Board".to_string(),
            spec: "16MB Flash".to_string(),
            package: "Module".to_string(),
            qty: 3,
            min_qty: 1,
            unit_cost: 6.33,
            currency: "USD".to_string(),
            location_id: None,
            location_code: Some("BIN-A1".to_string()),
            location_name: None,
            primary_supplier_id: None,
            primary_supplier_name: Some("Amazon".to_string()),
            mpn: String::new(),
            supplier_sku: String::new(),
            product_url: String::new(),
            datasheet_url: String::new(),
            barcode_value: "SKU:MCU-001".to_string(),
            projects_tags: "-".to_string(),
            description: String::new(),
            notes: String::new(),
            image_path: String::new(),
            pinout_image_path: String::new(),
            created_at: String::new(),
            updated_at: String::new(),
            extended_value: 18.99,
            low_stock: false,
        };
        let spec = build_part_label(&part, LabelPreset::Phomemo40x30, BarcodeStyle::Combo).unwrap();
        let png = render_label_png(&spec, PHOMEMO_DPI).unwrap();
        assert!(png.starts_with(&[0x89, 0x50, 0x4E, 0x47]));

        let luma = decode_image_bytes_to_luma(&png).unwrap();
        assert!(luma.width() <= M110_MAX_WIDTH_DOTS);
        let pkt = encode_m110_packet(&luma, 10, "gap", 1);
        assert!(pkt.starts_with(&[0x1B, 0x4E, 0x0D, 0x05, 0x1B, 0x4E, 0x04, 0x0A, 0x1F, 0x11, 0x0A, 0x1D, 0x76, 0x30, 0x00]));
        assert!(pkt.ends_with(&[0x1F, 0xF0, 0x05, 0x00, 0x1F, 0xF0, 0x03, 0x00]));
    }
}
