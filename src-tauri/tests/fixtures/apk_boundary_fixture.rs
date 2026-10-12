//! Small binary-XML/ZIP fixtures; no device, external executable or APK SDK.
use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

pub fn put_u32(buf: &mut [u8], at: usize, value: u32) {
    buf[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

pub fn document(chunks: &[Vec<u8>]) -> Vec<u8> {
    let mut out = vec![3, 0, 8, 0, 0, 0, 0, 0];
    for chunk in chunks {
        out.extend_from_slice(chunk);
    }
    let size = out.len() as u32;
    put_u32(&mut out, 4, size);
    out
}

fn utf8_len(out: &mut Vec<u8>, len: usize) {
    assert!(len <= 0x7fff);
    if len >= 0x80 {
        out.push(0x80 | (len >> 8) as u8);
    }
    out.push(len as u8);
}

pub fn pool(strings: &[&str], utf8: bool) -> Vec<u8> {
    let mut data = Vec::new();
    let mut offsets = Vec::new();
    for text in strings {
        offsets.push(data.len() as u32);
        let units: Vec<u16> = text.encode_utf16().collect();
        if utf8 {
            utf8_len(&mut data, units.len());
            utf8_len(&mut data, text.len());
            data.extend_from_slice(text.as_bytes());
            data.push(0);
        } else {
            if units.len() >= 0x8000 {
                data.extend_from_slice(&(0x8000 | (units.len() >> 16) as u16).to_le_bytes());
            }
            data.extend_from_slice(&(units.len() as u16).to_le_bytes());
            for unit in units {
                data.extend_from_slice(&unit.to_le_bytes());
            }
            data.extend_from_slice(&0u16.to_le_bytes());
        }
    }
    while data.len() % 4 != 0 {
        data.push(0);
    }
    let start = 28 + strings.len() * 4;
    let mut out = vec![1, 0, 28, 0];
    for value in [
        (start + data.len()) as u32,
        strings.len() as u32,
        0,
        if utf8 { 256 } else { 0 },
        start as u32,
        0,
    ] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    for offset in offsets {
        out.extend_from_slice(&offset.to_le_bytes());
    }
    out.extend_from_slice(&data);
    out
}

/// Each attribute is (name index, raw string index, typed type, typed data).
pub fn element(name: u32, attributes: &[(u32, u32, u8, u32)]) -> Vec<u8> {
    let mut out = vec![2, 1, 16, 0];
    for value in [
        (36 + attributes.len() * 20) as u32,
        1,
        u32::MAX,
        u32::MAX,
        name,
    ] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    for value in [20, 20, attributes.len() as u16, 0, 0, 0] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    for &(name, raw, kind, value) in attributes {
        for word in [u32::MAX, name, raw, 8 | ((kind as u32) << 24), value] {
            out.extend_from_slice(&word.to_le_bytes());
        }
    }
    out
}

pub fn end_element(name: u32) -> Vec<u8> {
    let mut out = vec![3, 1, 16, 0];
    for value in [24u32, 1, u32::MAX, u32::MAX, name] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

pub fn manifest(package: &str) -> Vec<u8> {
    document(&[
        pool(&["manifest", "package", package], true),
        element(0, &[(1, 2, 3, 2)]),
        end_element(0),
    ])
}

pub fn enormous_pool_count() -> Vec<u8> {
    let mut chunk = pool(&[], true);
    put_u32(&mut chunk, 8, u32::MAX);
    document(&[chunk])
}

fn repeated_pool(count: usize, text: &str) -> Vec<u8> {
    let one = pool(&[text], true);
    let start = 28 + count * 4;
    let mut chunk = one[..28].to_vec();
    chunk.resize(start, 0);
    chunk.extend_from_slice(&one[32..]);
    let size = chunk.len() as u32;
    put_u32(&mut chunk, 4, size);
    put_u32(&mut chunk, 8, count as u32);
    put_u32(&mut chunk, 20, start as u32);
    chunk
}

pub fn budget_case(case: &str) -> Vec<u8> {
    let (pool_chunk, node, repetitions) = match case {
        "axml-pool-bytes" => return document(&[repeated_pool(4096, &"x".repeat(32_760))]),
        "axml-string-count" => return document(&[repeated_pool(65_537, "")]),
        "axml-single-string" => return document(&[pool(&[&"x".repeat(1024 * 1024 + 1)], false)]),
        "axml-clone-bytes" => (
            pool(&["e", "v", &"x".repeat(32_760)], true),
            element(0, &vec![(1, 2, 3, 2); 1024]),
            1,
        ),
        "axml-element-count" => (pool(&["e"], true), element(0, &[]), 32_769),
        "axml-attribute-count" => (
            pool(&["e", "v", ""], true),
            element(0, &vec![(1, 2, 3, 2); 43_691]),
            3,
        ),
        "axml-chunk-count" => (pool(&[], true), vec![0xff, 0x7f, 8, 0, 8, 0, 0, 0], 131_072),
        _ => panic!("unknown AXML budget fixture {case}"),
    };
    let mut bytes = document(&[pool_chunk]);
    for _ in 0..repetitions {
        bytes.extend_from_slice(&node);
    }
    let size = bytes.len() as u32;
    put_u32(&mut bytes, 4, size);
    bytes
}

pub struct ApkFixture(pub PathBuf);
impl ApkFixture {
    pub fn new(manifest: &[u8], padding: usize, declared: Option<u32>) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::var_os("TMPDIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join(format!(
            "vrmanager-apk-boundary-{}-{}.apk",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let file = File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("AndroidManifest.xml", options).unwrap();
        zip.write_all(manifest).unwrap();
        let zeros = [0u8; 8192];
        let mut remaining = padding;
        while remaining != 0 {
            let count = remaining.min(zeros.len());
            zip.write_all(&zeros[..count]).unwrap();
            remaining -= count;
        }
        zip.finish().unwrap();
        if let Some(size) = declared {
            let bytes = std::fs::read(&path).unwrap();
            let central = bytes.windows(4).position(|b| b == b"PK\x01\x02").unwrap();
            let mut file = File::options().write(true).open(&path).unwrap();
            for at in [22, central + 24] {
                file.seek(SeekFrom::Start(at as u64)).unwrap();
                file.write_all(&size.to_le_bytes()).unwrap();
            }
        }
        Self(path)
    }
}
impl Drop for ApkFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
