//! Minimal Android binary XML (AXML) reader.
//!
//! Only what manifest inspection needs: the string pool, resource map and
//! start-element events with their attributes. Values that reference
//! resources are returned as `@0x7f...` so callers can decide what to do
//! rather than silently showing a number.
//!
//! Format reference (AOSP `ResourceTypes.h`), little-endian throughout:
//!   chunk header: u16 type, u16 header_size, u32 chunk_size
//!   types: 0x0003 string pool, 0x0100 start ns, 0x0101 end ns,
//!          0x0102 start element, 0x0103 end element, 0x0180 resource map

const CHUNK_STRING_POOL: u16 = 0x0001;
const CHUNK_XML: u16 = 0x0003;
const CHUNK_RESOURCE_MAP: u16 = 0x0180;
const CHUNK_START_ELEMENT: u16 = 0x0102;

const UTF8_FLAG: u32 = 1 << 8;

#[path = "axml_budget.rs"]
mod budget;
use budget::{Budget, MAX_CHUNKS, MAX_ELEMENTS};

// Attribute value types we care about.
const TYPE_REFERENCE: u8 = 0x01;
const TYPE_STRING: u8 = 0x03;
const TYPE_FLOAT: u8 = 0x04;
const TYPE_INT_DEC: u8 = 0x10;
const TYPE_INT_HEX: u8 = 0x11;
const TYPE_INT_BOOL: u8 = 0x12;

#[derive(Debug, Clone, PartialEq)]
pub struct Attribute {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Element {
    pub name: String,
    pub attributes: Vec<Attribute>,
}

impl Element {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|a| a.name == name)
            .map(|a| a.value.as_str())
    }
}

#[derive(Debug, Clone, Copy)]
struct Cursor<'a> {
    buf: &'a [u8],
}

impl<'a> Cursor<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf }
    }

    fn u16_at(&self, at: usize) -> Option<u16> {
        self.buf
            .get(at..at.checked_add(2)?)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32_at(&self, at: usize) -> Option<u32> {
        self.buf
            .get(at..at.checked_add(4)?)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
}

/// Decoded AXML document: the elements we saw, in document order.
#[derive(Debug, Clone, Default)]
pub struct Document {
    pub elements: Vec<Element>,
}

impl Document {
    pub fn find(&self, name: &str) -> Option<&Element> {
        self.elements.iter().find(|e| e.name == name)
    }

    pub fn find_all<'s>(&'s self, name: &'s str) -> impl Iterator<Item = &'s Element> + 's {
        self.elements.iter().filter(move |e| e.name == name)
    }
}

/// Parse bounded AXML. Invalid chunks fail the whole document, never return
/// partially decoded metadata from a malformed manifest.
pub fn parse(buf: &[u8]) -> Option<Document> {
    let cur = Cursor::new(buf);
    if cur.u16_at(0)? != CHUNK_XML || buf.len() > super::MAX_MANIFEST_BYTES {
        return None;
    }
    let end = usize::try_from(cur.u32_at(4)?).ok()?;
    let mut pos = usize::from(cur.u16_at(2)?);
    if end != buf.len() || pos < 8 || pos > end {
        return None;
    }

    let mut strings = Vec::new();
    let mut resource_ids = Vec::new();
    let mut doc = Document::default();
    let mut budget = Budget::new();
    let mut chunks = 0usize;
    while pos < end {
        chunks = chunks.checked_add(1)?;
        if chunks > MAX_CHUNKS {
            return None;
        }
        let chunk_type = cur.u16_at(pos)?;
        let header = usize::from(cur.u16_at(pos.checked_add(2)?)?);
        let size = usize::try_from(cur.u32_at(pos.checked_add(4)?)?).ok()?;
        let next = pos.checked_add(size)?;
        if header < 8 || header > size || next > end {
            return None;
        }
        let chunk = buf.get(pos..next)?;
        match chunk_type {
            CHUNK_STRING_POOL => strings = parse_string_pool(chunk, &mut budget)?,
            CHUNK_RESOURCE_MAP => {
                let data = chunk.get(header..)?;
                if data.len() % 4 != 0 || data.len() / 4 > strings.len() {
                    return None;
                }
                resource_ids.clear();
                resource_ids.try_reserve_exact(data.len() / 4).ok()?;
                resource_ids.extend(
                    data.chunks_exact(4)
                        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])),
                );
            }
            CHUNK_START_ELEMENT => {
                if doc.elements.len() >= MAX_ELEMENTS {
                    return None;
                }
                let element = parse_start_element(chunk, &strings, &resource_ids, &mut budget)?;
                doc.elements.try_reserve(1).ok()?;
                doc.elements.push(element);
            }
            _ => {}
        }
        pos = next;
    }
    Some(doc)
}

#[path = "axml_pool.rs"]
mod pool;
use pool::parse_string_pool;
#[cfg(test)]
use pool::read_utf16_string;

fn pool_get(strings: &[String], idx: u32) -> Option<&str> {
    if idx == u32::MAX {
        return None;
    }
    strings.get(usize::try_from(idx).ok()?).map(String::as_str)
}

fn parse_start_element(
    chunk: &[u8],
    strings: &[String],
    resource_ids: &[u32],
    budget: &mut Budget,
) -> Option<Element> {
    let cur = Cursor::new(chunk);
    let read_u16 = |at| cur.u16_at(at);
    let read_u32 = |at| cur.u32_at(at);
    // ResXMLTree_node is followed by the 20-byte ResXMLTree_attrExt.
    let header = usize::from(read_u16(2)?);
    if header < 16 {
        return None;
    }
    let ext = Cursor::new(chunk.get(header..)?);
    let name_idx = ext.u32_at(4)?;
    let attr_start = usize::from(ext.u16_at(8)?);
    let stride = usize::from(ext.u16_at(10)?);
    let attr_count = usize::from(ext.u16_at(12)?);
    let base = header.checked_add(attr_start)?;
    let end = base.checked_add(attr_count.checked_mul(stride)?)?;
    if attr_start < 20 || stride < 20 || end > chunk.len() {
        return None;
    }
    budget.attributes(attr_count)?;
    let name = budget.copy(pool_get(strings, name_idx)?)?;
    let mut attributes = Vec::new();
    attributes.try_reserve_exact(attr_count).ok()?;
    for i in 0..attr_count {
        let at = base.checked_add(i.checked_mul(stride)?)?;
        let attr_name_idx = read_u32(at + 4)?;
        let raw_value_idx = read_u32(at + 8)?;
        let typed_type = (read_u32(at + 12)? >> 24) as u8;
        let typed_data = read_u32(at + 16)?;

        let attr_name = pool_get(strings, attr_name_idx)
            .filter(|name| !name.is_empty())
            .or_else(|| {
                // Framework attributes often have an empty pool entry; fall back
                // to the resource map so `android:versionName` is still named.
                resource_ids
                    .get(attr_name_idx as usize)
                    .and_then(|id| framework_attr_name(*id))
            })?;
        let attr_name = budget.copy(attr_name)?;

        let value = match typed_type {
            TYPE_STRING => budget.copy(pool_get(
                strings,
                if raw_value_idx == u32::MAX {
                    typed_data
                } else {
                    raw_value_idx
                },
            )?)?,
            TYPE_INT_BOOL => budget.copy(if typed_data == 0 { "false" } else { "true" })?,
            TYPE_INT_DEC => budget.copy(&typed_data.to_string())?,
            TYPE_INT_HEX => budget.copy(&format!("0x{typed_data:x}"))?,
            TYPE_FLOAT => budget.copy(&f32::from_bits(typed_data).to_string())?,
            TYPE_REFERENCE => budget.copy(&format!("@0x{typed_data:08x}"))?,
            _ => match pool_get(strings, raw_value_idx) {
                Some(text) => budget.copy(text)?,
                None => budget.copy(&typed_data.to_string())?,
            },
        };

        attributes.push(Attribute {
            name: attr_name,
            value,
        });
    }

    Some(Element { name, attributes })
}

/// The handful of `android:` attribute resource IDs manifest parsing needs.
fn framework_attr_name(id: u32) -> Option<&'static str> {
    match id {
        0x0101_0003 => Some("name"),
        0x0101_0001 => Some("label"),
        0x0101_0002 => Some("icon"),
        0x0101_0021 => Some("versionCode"),
        0x0101_0022 => Some("versionName"),
        0x0101_0270 => Some("minSdkVersion"),
        0x0101_0271 => Some("targetSdkVersion"),
        0x0101_000c => Some("maxSdkVersion"),
        0x0101_0020 => Some("sharedUserId"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a tiny but structurally valid AXML document with a UTF-8 pool.
    fn synth_axml() -> Vec<u8> {
        let strings = [
            "manifest",
            "package",
            "com.example.app",
            "versionName",
            "1.4.2",
        ];

        // --- string pool ---
        let mut pool_strings: Vec<u8> = Vec::new();
        let mut offsets: Vec<u32> = Vec::new();
        for s in strings {
            offsets.push(pool_strings.len() as u32);
            let bytes = s.as_bytes();
            pool_strings.push(bytes.len() as u8); // char count
            pool_strings.push(bytes.len() as u8); // byte count
            pool_strings.extend_from_slice(bytes);
            pool_strings.push(0);
        }
        let header_size: u32 = 28;
        let strings_start = header_size + offsets.len() as u32 * 4;
        let pool_size = strings_start + pool_strings.len() as u32;

        let mut pool: Vec<u8> = Vec::new();
        pool.extend_from_slice(&CHUNK_STRING_POOL.to_le_bytes());
        pool.extend_from_slice(&(header_size as u16).to_le_bytes());
        pool.extend_from_slice(&pool_size.to_le_bytes());
        pool.extend_from_slice(&(strings.len() as u32).to_le_bytes()); // stringCount
        pool.extend_from_slice(&0u32.to_le_bytes()); // styleCount
        pool.extend_from_slice(&UTF8_FLAG.to_le_bytes()); // flags
        pool.extend_from_slice(&strings_start.to_le_bytes()); // stringsStart
        pool.extend_from_slice(&0u32.to_le_bytes()); // stylesStart
        for off in &offsets {
            pool.extend_from_slice(&off.to_le_bytes());
        }
        pool.extend_from_slice(&pool_strings);

        // --- start element <manifest package="com.example.app" versionName="1.4.2"> ---
        let attr_count = 2usize;
        let elem_size = (16 + 20 + attr_count * 20) as u32;
        let mut elem: Vec<u8> = Vec::new();
        elem.extend_from_slice(&CHUNK_START_ELEMENT.to_le_bytes());
        elem.extend_from_slice(&16u16.to_le_bytes()); // header size
        elem.extend_from_slice(&elem_size.to_le_bytes());
        elem.extend_from_slice(&1u32.to_le_bytes()); // line number
        elem.extend_from_slice(&u32::MAX.to_le_bytes()); // comment
        elem.extend_from_slice(&u32::MAX.to_le_bytes()); // ns
        elem.extend_from_slice(&0u32.to_le_bytes()); // name -> "manifest"
        elem.extend_from_slice(&20u16.to_le_bytes()); // attributeStart
        elem.extend_from_slice(&20u16.to_le_bytes()); // attributeSize
        elem.extend_from_slice(&(attr_count as u16).to_le_bytes()); // attributeCount
        elem.extend_from_slice(&0u16.to_le_bytes()); // idIndex
        elem.extend_from_slice(&0u16.to_le_bytes()); // classIndex
        elem.extend_from_slice(&0u16.to_le_bytes()); // styleIndex

        // attribute: package="com.example.app"
        for (name_idx, value_idx) in [(1u32, 2u32), (3u32, 4u32)] {
            elem.extend_from_slice(&u32::MAX.to_le_bytes()); // ns
            elem.extend_from_slice(&name_idx.to_le_bytes()); // name
            elem.extend_from_slice(&value_idx.to_le_bytes()); // raw value
            elem.extend_from_slice(&((TYPE_STRING as u32) << 24).to_le_bytes()); // typed size/type
            elem.extend_from_slice(&value_idx.to_le_bytes()); // typed data
        }

        let total = 8 + pool.len() + elem.len();
        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(&CHUNK_XML.to_le_bytes());
        out.extend_from_slice(&8u16.to_le_bytes());
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(&pool);
        out.extend_from_slice(&elem);
        out
    }

    #[test]
    fn parses_synthetic_manifest() {
        let buf = synth_axml();
        let doc = parse(&buf).expect("should recognise AXML");
        let manifest = doc.find("manifest").expect("manifest element");
        assert_eq!(manifest.attr("package"), Some("com.example.app"));
        assert_eq!(manifest.attr("versionName"), Some("1.4.2"));
    }

    #[test]
    fn rejects_non_axml_data() {
        assert!(parse(b"<?xml version=\"1.0\"?><manifest/>").is_none());
        assert!(parse(&[]).is_none());
    }

    #[test]
    fn truncated_data_does_not_panic() {
        let buf = synth_axml();
        for cut in [9usize, 20, 40, buf.len() / 2] {
            let _ = parse(&buf[..cut.min(buf.len())]);
        }
    }

    #[test]
    fn framework_attribute_ids_are_mapped() {
        assert_eq!(framework_attr_name(0x0101_0021), Some("versionCode"));
        assert_eq!(framework_attr_name(0x0101_0022), Some("versionName"));
        assert_eq!(framework_attr_name(0xdead_beef), None);
    }

    #[test]
    fn utf16_pool_entries_decode() {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&3u16.to_le_bytes());
        for unit in "abc".encode_utf16() {
            buf.extend_from_slice(&unit.to_le_bytes());
        }
        buf.extend_from_slice(&0u16.to_le_bytes());
        assert_eq!(
            read_utf16_string(&buf, 0, &mut Budget::new()).unwrap(),
            "abc"
        );
    }
}
