//! Parser for the standard BLE Heart Rate Measurement characteristic (0x2A37).

#[derive(Debug, Clone, PartialEq)]
pub struct HrMeasurement {
    pub bpm: u16,
    /// R-R intervals in milliseconds (the wire unit is 1/1024 s).
    pub rr_ms: Vec<u16>,
}

/// Flags byte: bit0 = BPM is UINT16, bit3 = Energy Expended present (2 bytes),
/// bit4 = one or more R-R intervals follow (UINT16 each).
pub fn parse(data: &[u8]) -> Option<HrMeasurement> {
    let flags = *data.first()?;
    let mut i = 1;

    let bpm = if flags & 0x01 != 0 {
        let v = u16::from_le_bytes([*data.get(i)?, *data.get(i + 1)?]);
        i += 2;
        v
    } else {
        let v = *data.get(i)? as u16;
        i += 1;
        v
    };

    if flags & 0x08 != 0 {
        i += 2;
    }

    let mut rr_ms = Vec::new();
    if flags & 0x10 != 0 {
        while i + 1 < data.len() {
            let raw = u16::from_le_bytes([data[i], data[i + 1]]) as u32;
            rr_ms.push((raw * 1000 / 1024) as u16);
            i += 2;
        }
    }

    Some(HrMeasurement { bpm, rr_ms })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uint8_bpm() {
        let m = parse(&[0x00, 68]).unwrap();
        assert_eq!(m.bpm, 68);
        assert!(m.rr_ms.is_empty());
    }

    #[test]
    fn uint16_bpm() {
        let m = parse(&[0x01, 0x2C, 0x01]).unwrap();
        assert_eq!(m.bpm, 300);
    }

    #[test]
    fn uint8_bpm_with_rr() {
        // 1024/1024 s = 1000 ms, 512/1024 s = 500 ms
        let m = parse(&[0x10, 72, 0x00, 0x04, 0x00, 0x02]).unwrap();
        assert_eq!(m.bpm, 72);
        assert_eq!(m.rr_ms, vec![1000, 500]);
    }

    #[test]
    fn energy_expended_is_skipped_before_rr() {
        let m = parse(&[0x19, 0x50, 0x00, 0xAA, 0xBB, 0x00, 0x04]).unwrap();
        assert_eq!(m.bpm, 80);
        assert_eq!(m.rr_ms, vec![1000]);
    }

    #[test]
    fn truncated_packets_are_rejected() {
        assert!(parse(&[]).is_none());
        assert!(parse(&[0x00]).is_none());
        assert!(parse(&[0x01, 0x40]).is_none());
    }
}
