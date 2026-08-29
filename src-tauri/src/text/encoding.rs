use encoding_rs::{GB18030, UTF_16BE, UTF_16LE, UTF_8};

pub(crate) fn decode_text(bytes: &[u8]) -> (String, String) {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        let (text, _, _) = UTF_8.decode(&bytes[3..]);
        return (text.into_owned(), "utf-8".to_string());
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let (text, _, _) = UTF_16LE.decode(&bytes[2..]);
        return (text.into_owned(), "utf-16le".to_string());
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let (text, _, _) = UTF_16BE.decode(&bytes[2..]);
        return (text.into_owned(), "utf-16be".to_string());
    }
    let (utf8, _, had_errors) = UTF_8.decode(bytes);
    if !had_errors {
        return (utf8.into_owned(), "utf-8".to_string());
    }
    let (gb18030, _, _) = GB18030.decode(bytes);
    (gb18030.into_owned(), "gb18030".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_plain_utf8() {
        let (text, encoding) = decode_text("第一章 雪鹰领".as_bytes());
        assert_eq!(encoding, "utf-8");
        assert_eq!(text, "第一章 雪鹰领");
    }

    #[test]
    fn strips_utf8_bom() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice("第一章".as_bytes());
        let (text, encoding) = decode_text(&bytes);
        assert_eq!(encoding, "utf-8");
        assert_eq!(text, "第一章");
    }

    #[test]
    fn decodes_utf16le_bom() {
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend_from_slice(&[0x2D, 0x4E]); // "中" U+4E2D
        let (text, encoding) = decode_text(&bytes);
        assert_eq!(encoding, "utf-16le");
        assert_eq!(text, "中");
    }

    #[test]
    fn decodes_utf16be_bom() {
        let mut bytes = vec![0xFE, 0xFF];
        bytes.extend_from_slice(&[0x4E, 0x2D]); // "中" U+4E2D
        let (text, encoding) = decode_text(&bytes);
        assert_eq!(encoding, "utf-16be");
        assert_eq!(text, "中");
    }

    #[test]
    fn falls_back_to_gb18030_for_non_utf8() {
        // Self-verify through the encoder instead of hardcoding GBK bytes.
        let (encoded, _, _) = GB18030.encode("雪鹰领");
        assert!(encoded.len() >= 6);
        let (text, encoding) = decode_text(&encoded);
        assert_eq!(encoding, "gb18030");
        assert_eq!(text, "雪鹰领");
    }

    #[test]
    fn gb18030_handles_four_byte_sequences() {
        // U+20BB7 ("𠮷") requires the four-byte GB18030 encoding.
        let (encoded, _, _) = GB18030.encode("𠮷");
        assert!(encoded.len() >= 4);
        let (text, encoding) = decode_text(&encoded);
        assert_eq!(encoding, "gb18030");
        assert_eq!(text, "𠮷");
    }
}
