//! Read-only hex dump formatting for opaque MQTT payloads.

/// Classic hex dump: `offset | hex bytes | ASCII` (16 bytes per line).
pub fn format_hex_dump(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(bytes.len() * 4);
    for (row, chunk) in bytes.chunks(16).enumerate() {
        if row > 0 {
            out.push('\n');
        }
        let offset = row * 16;
        out.push_str(&format!("{offset:08x}  "));
        for i in 0..16 {
            if i == 8 {
                out.push(' ');
            }
            if let Some(b) = chunk.get(i) {
                out.push_str(&format!("{b:02x} "));
            } else {
                out.push_str("   ");
            }
        }
        out.push(' ');
        for b in chunk {
            let c = if (0x20..=0x7e).contains(b) {
                *b as char
            } else {
                '.'
            };
            out.push(c);
        }
    }
    out
}

/// True when `bytes` is valid UTF-8 (empty counts as valid).
pub fn is_utf8(bytes: &[u8]) -> bool {
    std::str::from_utf8(bytes).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dumps_ascii_and_binary() {
        let s = format_hex_dump(b"Hi\0!");
        assert!(s.contains("48 69 00 21"));
        assert!(s.contains("Hi.!"));
        assert!(s.starts_with("00000000"));
    }

    #[test]
    fn empty_is_empty() {
        assert!(format_hex_dump(&[]).is_empty());
        assert!(is_utf8(&[]));
        assert!(!is_utf8(&[0xff, 0xfe]));
    }

    #[test]
    fn second_row_offsets() {
        let mut buf = vec![0u8; 17];
        buf[16] = b'Z';
        let s = format_hex_dump(&buf);
        assert!(s.contains('\n'));
        assert!(s.contains("00000010"));
    }
}
