use anyhow::{anyhow, Result};

/// ISO/IEC 15417 Code 128 bar/space width patterns (codes 0..=106).
/// Codes 0..=105 have 6 alternating bar/space widths summing to 11 modules.
/// Code 106 (Stop) has 7 alternating widths summing to 13 modules.
const PATTERNS: [&str; 107] = [
    "212222", "222122", "222221", "121223", "121322", "131222", "122213", "122312",
    "132212", "221213", "221312", "231212", "112232", "122132", "122231", "113222",
    "123122", "123221", "223211", "221132", "221231", "213212", "223112", "312131",
    "311222", "321122", "321221", "312212", "322112", "322211", "212123", "212321",
    "232121", "111323", "131123", "131321", "112313", "132113", "132311", "211313",
    "231113", "231311", "112133", "112331", "132131", "113123", "113321", "133121",
    "313121", "211331", "231131", "213113", "213311", "213131", "311123", "311321",
    "331121", "312113", "312311", "332111", "314111", "221411", "431111", "111224",
    "111422", "121124", "121421", "141122", "141221", "112214", "112412", "122114",
    "122411", "142112", "142211", "241211", "221114", "413111", "241112", "134111",
    "111242", "121142", "121241", "114212", "124112", "124211", "411212", "421112",
    "421211", "212141", "214121", "412121", "111143", "111341", "131141", "114113",
    "114311", "411113", "411311", "113141", "114131", "311141", "411131", "211412",
    "211214", "211232", "2331112",
];

#[derive(Debug, Clone)]
pub struct Code128Run {
    /// Start module index (after quiet zone)
    pub x_mod: usize,
    /// Width in modules
    pub w_mod: usize,
}

#[derive(Debug, Clone)]
pub struct Code128Encoded {
    /// Total width in modules INCLUDING 10-module quiet zones on both sides
    pub total_modules: usize,
    /// Black bar runs (x_mod, w_mod)
    pub bars: Vec<Code128Run>,
    #[allow(dead_code)]
    pub checksum: usize,
}

/// Encodes printable ASCII (32..=126) using Code 128 Set B with 10-module quiet zones.
pub fn encode_code128b(text: &str) -> Result<Code128Encoded> {
    if text.is_empty() {
        return Err(anyhow!("Code 128 input cannot be empty"));
    }

    let mut values: Vec<usize> = Vec::with_capacity(text.len() + 3);
    values.push(104); // Start Code B

    let mut weighted_sum: usize = 104;
    for (idx, b) in text.bytes().enumerate() {
        if !(32..=126).contains(&b) {
            return Err(anyhow!(
                "Unsupported Code 128B byte 0x{:02X}; only printable ASCII 32..=126 is supported",
                b
            ));
        }
        let val = (b - 32) as usize;
        values.push(val);
        weighted_sum += (idx + 1) * val;
    }

    let checksum = weighted_sum % 103;
    values.push(checksum);
    values.push(106); // Stop

    let quiet = 10usize;
    let mut cursor = quiet;
    let mut bars = Vec::new();

    for sym in values {
        let pat = PATTERNS[sym];
        for (i, ch) in pat.bytes().enumerate() {
            let w = (ch - b'0') as usize;
            let is_bar = i % 2 == 0;
            if is_bar {
                bars.push(Code128Run {
                    x_mod: cursor,
                    w_mod: w,
                });
            }
            cursor += w;
        }
    }

    let total_modules = cursor + quiet;
    Ok(Code128Encoded {
        total_modules,
        bars,
        checksum,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code128_patterns_valid_sums() {
        for (idx, p) in PATTERNS.iter().enumerate() {
            let sum: usize = p.bytes().map(|b| (b - b'0') as usize).sum();
            if idx < 106 {
                assert_eq!(sum, 11, "Pattern {} sum mismatch", idx);
                assert_eq!(p.len(), 6);
            } else {
                assert_eq!(sum, 13, "Stop pattern sum mismatch");
                assert_eq!(p.len(), 7);
            }
        }
    }

    #[test]
    fn encode_mcu_001() {
        let enc = encode_code128b("MCU-001").unwrap();
        // Start B (11) + 7 chars (77) + checksum (11) + Stop (13) + 2*10 quiet = 132
        assert_eq!(enc.total_modules, 132);
        assert!(!enc.bars.is_empty());
        assert_eq!(enc.bars[0].x_mod, 10);
    }
}
