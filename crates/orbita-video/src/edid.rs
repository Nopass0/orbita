//! Minimal EDID (Extended Display Identification Data) parser.
//!
//! Reads the 128-byte base block: header, manufacturer id, product code,
//! serial, and the preferred timing descriptor. Enough for the kernel to
//! know *which monitor* is attached without firmware help.

use alloc::string::String;

/// Parsed contents of an EDID base block.
#[derive(Debug, Clone, PartialEq)]
pub struct EdidInfo {
    pub manufacturer_id: [u8; 3],
    pub product_code: u16,
    pub serial: u32,
    pub year: u16,
    pub week: u8,
    /// Preferred (detailed) timing, when present.
    pub preferred: Option<EdidTiming>,
}

/// A single display timing from a detailed timing descriptor.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct EdidTiming {
    pub pixel_clock_khz: u32,
    pub width: u16,
    pub height: u16,
    pub refresh_hz: u8,
}

/// Checks the 8-byte EDID header (`00 FF FF FF FF FF FF 00`).
fn header_valid(data: &[u8]) -> bool {
    data.len() >= 8
        && data[0] == 0x00
        && data[1..7].iter().all(|&b| b == 0xFF)
        && data[7] == 0x00
}

/// Checks the EDID checksum (sum of all 128 bytes mod 256 == 0).
fn checksum_valid(data: &[u8]) -> bool {
    data.len() >= 128 && data[..128].iter().fold(0u8, |a, b| a.wrapping_add(*b)) == 0
}

/// Parses a 128-byte EDID base block. Returns `None` on bad header or
/// checksum.
pub fn parse(data: &[u8]) -> Option<EdidInfo> {
    if !header_valid(data) || !checksum_valid(data) {
        return None;
    }

    // Manufacturer id: three 5-bit letters packed into bytes 8..10.
    let mraw = ((data[8] as u16) << 8) | data[9] as u16;
    let letter = |shift: u16| -> u8 { (b'A' - 1) + ((mraw >> shift) & 0x1F) as u8 };
    let manufacturer_id = [letter(10), letter(5), letter(0)];

    let product_code = u16::from_le_bytes([data[10], data[11]]);
    let serial = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
    let week = data[16];
    let year = 1990 + data[17] as u16;

    // Descriptor blocks at 0x36, 0x48, 0x5A, 0x6C; the first two are
    // usually detailed timings. The preferred one is the first valid.
    let mut preferred = None;
    for &off in &[0x36usize, 0x48, 0x5A, 0x6C] {
        if data[off + 1] | data[off + 2] == 0 {
            continue; // monitor-range/serial/text descriptor, not a timing
        }
        preferred = parse_timing(&data[off..off + 18]);
        if preferred.is_some() {
            break;
        }
    }

    Some(EdidInfo {
        manufacturer_id,
        product_code,
        serial,
        year,
        week,
        preferred,
    })
}

/// Parses one 18-byte detailed timing descriptor.
fn parse_timing(d: &[u8]) -> Option<EdidTiming> {
    let pixel_clock_khz = u16::from_le_bytes([d[0], d[1]]) as u32 * 10;
    if pixel_clock_khz == 0 {
        return None;
    }
    let hactive = u16::from(d[2]) | (u16::from(d[4] & 0xF0) << 4);
    let vactive = u16::from(d[5]) | (u16::from(d[7] & 0xF0) << 4);
    let hblank = u16::from(d[3]) | (u16::from(d[4] & 0x0F) << 8);
    let vblank = u16::from(d[6]) | (u16::from(d[7] & 0x0F) << 8);

    let htotal = hactive + hblank;
    let vtotal = vactive + vblank;
    let refresh_hz = if htotal != 0 && vtotal != 0 {
        ((pixel_clock_khz * 1000) / (htotal as u32 * vtotal as u32)).clamp(1, 255) as u8
    } else {
        60
    };

    Some(EdidTiming {
        pixel_clock_khz,
        width: hactive,
        height: vactive,
        refresh_hz,
    })
}

/// Formats the three-letter manufacturer id as text (e.g. "BNQ").
pub fn manufacturer_text(id: &[u8; 3]) -> String {
    id.iter().map(|&c| c as char).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use alloc::vec::Vec;

    /// Hand-built 128-byte EDID: header, "AAA" vendor, timing 640x480.
    fn sample_edid() -> Vec<u8> {
        let mut d = vec![0u8; 128];
        d[0] = 0x00;
        d[1..7].fill(0xFF);
        d[7] = 0x00;
        // "AAA" = 0b00001_00001_00001 -> 0x04, 0x21
        d[8] = 0x04;
        d[9] = 0x21;
        d[10..12].copy_from_slice(&1234u16.to_le_bytes());
        d[16] = 5;
        d[17] = 34; // 2024
        // Detailed timing at 0x36: pixel clock 25.175 MHz, 640x480@60.
        let t = &mut d[0x36..0x36 + 18];
        t[0] = 0x39; // 0x3997 = 25175 * 10 low byte 0x97? build explicitly below
        t[1] = 0x97;
        t[2] = 0x80; // hactive low
        t[3] = 0x20; // hblank low = 320
        t[4] = 0x20; // hactive high nibble
        t[5] = 0xE0;
        t[6] = 0x12;
        t[7] = 0x10; // vactive high nibble (480 = 0x1E0)
        // fix checksum
        let sum: u8 = d[..127].iter().fold(0u8, |a, b| a.wrapping_add(*b));
        d[127] = sum.wrapping_neg();
        d
    }

    #[test]
    fn parses_header_vendor_and_timing() {
        let edid = parse(&sample_edid()).expect("valid edid");
        assert_eq!(edid.manufacturer_id, [b'A', b'A', b'A']);
        assert_eq!(edid.product_code, 1234);
        assert_eq!(edid.year, 2024);
        let t = edid.preferred.expect("timing present");
        assert_eq!(t.width, 640);
        assert_eq!(t.height, 480);
    }

    #[test]
    fn rejects_bad_checksum() {
        let mut d = sample_edid();
        d[20] ^= 0xFF;
        assert!(parse(&d).is_none());
    }

    #[test]
    fn rejects_bad_header() {
        let mut d = sample_edid();
        d[3] = 0x00;
        // re-fix checksum so only the header is wrong
        let sum: u8 = d[..127].iter().fold(0u8, |a, b| a.wrapping_add(*b));
        d[127] = sum.wrapping_neg();
        assert!(parse(&d).is_none());
    }

    #[test]
    fn manufacturer_text_roundtrip() {
        assert_eq!(manufacturer_text(&[b'B', b'N', b'Q']), "BNQ");
    }
}
