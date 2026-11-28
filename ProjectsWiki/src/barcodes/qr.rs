use anyhow::Result;
use qrcode::{Color, EcLevel, QrCode};

#[derive(Debug, Clone)]
pub struct QrMatrix {
    /// Total side length in modules INCLUDING 4-module quiet zone on each side
    pub size_modules: usize,
    /// Dark modules as (col, row) including quiet zone offset
    pub dark_modules: Vec<(usize, usize)>,
}

pub fn encode_qr(data: &str) -> Result<QrMatrix> {
    let code = QrCode::with_error_correction_level(data.as_bytes(), EcLevel::M)?;
    let w = code.width();
    let quiet = 4usize;
    let size_modules = w + quiet * 2;
    let colors = code.to_colors();
    let mut dark_modules = Vec::new();
    for r in 0..w {
        for c in 0..w {
            if colors[r * w + c] == Color::Dark {
                dark_modules.push((c + quiet, r + quiet));
            }
        }
    }
    Ok(QrMatrix {
        size_modules,
        dark_modules,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qr_encodes_sku() {
        let m = encode_qr("SKU:MCU-001").unwrap();
        assert!(m.size_modules >= 29);
        assert!(!m.dark_modules.is_empty());
        assert!(m.dark_modules.iter().all(|&(c, r)| c >= 4 && r >= 4));
    }
}
