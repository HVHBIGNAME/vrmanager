//! Inspection policies, independent of input length and repeated pool indices.
pub(super) const MAX_STRINGS: usize = 65_536;
pub(super) const MAX_ELEMENTS: usize = 32_768;
pub(super) const MAX_CHUNKS: usize = 131_072;
pub(super) const MAX_STRING_BYTES: usize = 1024 * 1024;
const MAX_DECODED_BYTES: usize = 16 * 1024 * 1024;
const MAX_ATTRIBUTES: usize = 131_072;

/// Counts pool text AND every name/value copied into the output document.
/// Count limits separately bound Vec storage, including empty strings.
pub(super) struct Budget {
    text_left: usize,
    attributes_left: usize,
}
impl Budget {
    pub(super) fn new() -> Self {
        Self {
            text_left: MAX_DECODED_BYTES,
            attributes_left: MAX_ATTRIBUTES,
        }
    }

    pub(super) fn attributes(&mut self, count: usize) -> Option<()> {
        self.attributes_left = self.attributes_left.checked_sub(count)?;
        Some(())
    }

    pub(super) fn string(&mut self, size: usize) -> Option<String> {
        if size > MAX_STRING_BYTES {
            return None;
        }
        self.text_left = self.text_left.checked_sub(size)?;
        let mut out = String::new();
        out.try_reserve_exact(size).ok()?;
        Some(out)
    }

    pub(super) fn copy(&mut self, text: &str) -> Option<String> {
        let mut out = self.string(text.len())?;
        out.push_str(text);
        Some(out)
    }
}
