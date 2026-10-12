//! String-pool offsets are validated before any count-driven allocation.
use super::budget::{Budget, MAX_STRINGS, MAX_STRING_BYTES};
use super::{Cursor, UTF8_FLAG};

pub(super) fn parse_string_pool(chunk: &[u8], budget: &mut Budget) -> Option<Vec<String>> {
    let cur = Cursor::new(chunk);
    let header = usize::from(cur.u16_at(2)?);
    let count = usize::try_from(cur.u32_at(8)?).ok()?;
    let styles = usize::try_from(cur.u32_at(12)?).ok()?;
    let flags = cur.u32_at(16)?;
    let start = usize::try_from(cur.u32_at(20)?).ok()?;
    let styles_start = usize::try_from(cur.u32_at(24)?).ok()?;
    let table_end = header.checked_add(count.checked_add(styles)?.checked_mul(4)?)?;
    let strings_end = if styles_start == 0 {
        chunk.len()
    } else {
        styles_start
    };
    if header < 28
        || count > MAX_STRINGS
        || styles > count
        || table_end > start
        || start > strings_end
        || strings_end > chunk.len()
        || (styles == 0 && styles_start != 0)
        || (styles != 0 && styles_start == 0)
    {
        return None;
    }
    let string_offsets_end = header.checked_add(count.checked_mul(4)?)?;
    let offsets = chunk.get(header..string_offsets_end)?;
    let data = chunk.get(start..strings_end)?;
    let offset = |b: &[u8]| usize::try_from(u32::from_le_bytes([b[0], b[1], b[2], b[3]])).ok();
    for entry in offsets.chunks_exact(4) {
        if offset(entry)? >= data.len() {
            return None;
        }
    }
    for entry in chunk.get(string_offsets_end..table_end)?.chunks_exact(4) {
        let off = offset(entry)?;
        // NO_ENTRY is allowed for unstyled strings within a styled pool.
        if off != u32::MAX as usize
            && (off % 4 != 0 || styles_start.checked_add(off)?.checked_add(4)? > chunk.len())
        {
            return None;
        }
    }
    let mut out = Vec::new();
    out.try_reserve_exact(count).ok()?;
    for entry in offsets.chunks_exact(4) {
        out.push(if flags & UTF8_FLAG != 0 {
            read_utf8_string(data, offset(entry)?, budget)?
        } else {
            read_utf16_string(data, offset(entry)?, budget)?
        });
    }
    Some(out)
}

fn utf8_length(buf: &[u8], pos: &mut usize) -> Option<usize> {
    let first = *buf.get(*pos)?;
    *pos = pos.checked_add(1)?;
    if first & 0x80 == 0 {
        return Some(usize::from(first));
    }
    let second = *buf.get(*pos)?;
    *pos = pos.checked_add(1)?;
    Some((usize::from(first & 0x7f) << 8) | usize::from(second))
}

fn read_utf8_string(buf: &[u8], at: usize, budget: &mut Budget) -> Option<String> {
    let mut pos = at;
    let char_len = utf8_length(buf, &mut pos)?;
    let byte_len = utf8_length(buf, &mut pos)?;
    let end = pos.checked_add(byte_len)?;
    if *buf.get(end)? != 0 {
        return None;
    }
    let text = std::str::from_utf8(buf.get(pos..end)?).ok()?;
    if text.encode_utf16().count() != char_len {
        return None;
    }
    budget.copy(text)
}

pub(super) fn read_utf16_string(buf: &[u8], at: usize, budget: &mut Budget) -> Option<String> {
    let cur = Cursor::new(buf);
    let w0 = cur.u16_at(at)?;
    let mut p = at.checked_add(2)?;
    let len = if w0 & 0x8000 != 0 {
        let w1 = usize::from(cur.u16_at(p)?);
        p = p.checked_add(2)?;
        ((w0 as usize & 0x7fff) << 16) | w1
    } else {
        w0 as usize
    };
    let end = p.checked_add(len.checked_mul(2)?)?;
    if len > MAX_STRING_BYTES || cur.u16_at(end)? != 0 {
        return None;
    }
    let bytes = buf.get(p..end)?;
    // Count UTF-8 bytes first; never materialize a count-sized Vec<u16>.
    // The prior UTF-16 replacement policy is preserved.
    let chars = || {
        char::decode_utf16(
            bytes
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes([b[0], b[1]])),
        )
        .map(|ch| ch.unwrap_or(char::REPLACEMENT_CHARACTER))
    };
    let byte_len = chars().try_fold(0usize, |sum, ch| sum.checked_add(ch.len_utf8()))?;
    let mut out = budget.string(byte_len)?;
    out.extend(chars());
    Some(out)
}
