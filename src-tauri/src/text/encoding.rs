use encoding_rs::{GBK, UTF_8};

pub(crate) fn decode_text(bytes: &[u8]) -> (String, String) {
    let (utf8, _, had_errors) = UTF_8.decode(bytes);
    if !had_errors {
        return (utf8.into_owned(), "utf-8".to_string());
    }
    let (gbk, _, _) = GBK.decode(bytes);
    (gbk.into_owned(), "gbk".to_string())
}
