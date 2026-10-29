//! Xiaomi DisplayFeature diagnostic decoding. This is HAL state, not a
//! measurement of panel output or color accuracy.
const KEY: &[u8; 6] = b"MIDISP";

pub(super) fn decode(raw: &[u8]) -> Result<String, String> {
    if raw.starts_with(b"DF Param :") {
        return String::from_utf8(raw.to_vec()).map_err(|e| e.to_string());
    }
    let mut decoded = Vec::with_capacity(raw.len());
    for (i, &c) in raw.iter().enumerate() {
        if c == b'\n' {
            decoded.push(c);
            continue;
        }
        if !(32..=126).contains(&c) {
            return Err("invalid display dump encoding".into());
        }
        let delta = if i % 2 == 0 {
            KEY[i % 6] as i32
        } else {
            -(KEY[i % 6] as i32)
        };
        decoded.push(((c as i32 - 32 + delta).rem_euclid(95) + 32) as u8);
    }
    let text = String::from_utf8(decoded).map_err(|e| e.to_string())?;
    if !text.starts_with("DF Param :") {
        return Err("unrecognized display dump".into());
    }
    Ok(text)
}

pub(super) fn expert(raw: &[u8]) -> Result<[i32; 10], String> {
    let text = decode(raw)?;
    let section = text
        .split_once("\nExpert:\n")
        .ok_or("missing expert readback")?
        .1;
    let line = section.lines().next().ok_or("missing expert values")?;
    let mut tokens = line.split_whitespace();
    let mut values = [0; 10];
    for (i, name) in [
        "Enable:",
        "Gamut:",
        "R:",
        "G:",
        "B:",
        "H:",
        "S:",
        "V:",
        "Contrast:",
        "Gamma:",
    ]
    .iter()
    .enumerate()
    {
        if tokens.next() != Some(*name) {
            return Err("invalid expert readback fields".into());
        }
        values[i] = tokens
            .next()
            .ok_or("missing expert value")?
            .parse()
            .map_err(|_| "invalid expert value")?;
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decodes_device_header() {
        assert_eq!(decode(b"V0;:mcsW;$\n").unwrap(), "DF Param :\n");
    }
    #[test]
    fn decodes_entire_stream_without_resetting_at_newlines() {
        let plain = b"DF Param :\n\nExpert:\nEnable: 1  Gamut: 2  R: 200  G: 255  B: 255  H: 0  S: 0  V: 0  Contrast: 50  Gamma: 262\n";
        let encoded: Vec<u8> = plain
            .iter()
            .enumerate()
            .map(|(i, &c)| {
                if c == b'\n' {
                    return c;
                }
                let delta = if i % 2 == 0 {
                    -(KEY[i % 6] as i32)
                } else {
                    KEY[i % 6] as i32
                };
                ((c as i32 - 32 + delta).rem_euclid(95) + 32) as u8
            })
            .collect();
        assert_eq!(
            expert(&encoded).unwrap(),
            [1, 2, 200, 255, 255, 0, 0, 0, 50, 262]
        );
    }
    #[test]
    fn rejects_missing_or_corrupt_values() {
        assert!(expert(b"DF Param :\nExpert:\nEnable: 1 Gamut: 2 R: 200").is_err());
        assert!(decode(b"\xff").is_err());
    }
}
