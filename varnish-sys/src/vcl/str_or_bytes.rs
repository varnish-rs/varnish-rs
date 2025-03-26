use std::str::from_utf8;

#[derive(Debug)]
pub enum StrOrBytes<'a> {
    Utf8(&'a str),
    Bytes(&'a [u8]),
}

impl<'a> From<&'a str> for StrOrBytes<'a> {
    fn from(value: &'a str) -> Self {
        StrOrBytes::Utf8(value)
    }
}

impl<'a> From<&'a String> for StrOrBytes<'a> {
    fn from(value: &'a String) -> Self {
        StrOrBytes::Utf8(value.as_str())
    }
}

impl<'a> From<&'a [u8]> for StrOrBytes<'a> {
    fn from(value: &'a [u8]) -> Self {
        from_utf8(value).map_or_else(|_| StrOrBytes::Bytes(value), StrOrBytes::Utf8)
    }
}

impl AsRef<[u8]> for StrOrBytes<'_> {
    fn as_ref(&self) -> &[u8] {
        match self {
            StrOrBytes::Utf8(s) => s.as_bytes(),
            StrOrBytes::Bytes(b) => b,
        }
    }
}
