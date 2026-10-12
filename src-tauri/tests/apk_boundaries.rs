//! Hostile sizes run only in a child with an allocator tripwire, never real OOM.
use std::alloc::{GlobalAlloc, Layout, System};
use std::io::Write;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use vrmanager_lib::core::apk::{axml, inspect_apk};
use vrmanager_lib::error::CoreError;

#[path = "fixtures/apk_boundary_fixture.rs"]
mod fixture;
use fixture::*;
#[path = "fixtures/axml_budget_cases.rs"]
mod axml_budget_cases;
#[path = "fixtures/axml_boundary_cases.rs"]
mod axml_cases;
#[path = "fixtures/apk_metadata_cases.rs"]
mod metadata_cases;

static ALLOCATION_LIMIT: AtomicUsize = AtomicUsize::new(usize::MAX);
static TOTAL_ALLOCATION_BUDGET: AtomicUsize = AtomicUsize::new(usize::MAX);
struct GuardedAllocator;
fn guard(size: usize) {
    let single = size > ALLOCATION_LIMIT.load(Ordering::Relaxed);
    let total = TOTAL_ALLOCATION_BUDGET.load(Ordering::Relaxed) != usize::MAX
        && TOTAL_ALLOCATION_BUDGET
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |left| {
                left.checked_sub(size)
            })
            .is_err();
    if single || total {
        // Avoid reentrant logging allocations after the tripwire fired.
        ALLOCATION_LIMIT.store(usize::MAX, Ordering::Relaxed);
        TOTAL_ALLOCATION_BUDGET.store(usize::MAX, Ordering::Relaxed);
        let _ = writeln!(
            std::io::stderr().lock(),
            "BLOCKED_ALLOCATION_BYTES={size}; cumulative_budget_exceeded={total}"
        );
        std::process::exit(86);
    }
}
unsafe impl GlobalAlloc for GuardedAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        guard(layout.size());
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        guard(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        guard(size);
        unsafe { System.realloc(ptr, layout, size) }
    }
}
#[global_allocator]
static ALLOCATOR: GuardedAllocator = GuardedAllocator;

fn guarded_child(case: &str) {
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "guarded_rejection_probe",
            "--ignored",
            "--nocapture",
        ])
        .env("VRMANAGER_APK_GUARD_CASE", case)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{case}: {:?}\n{}\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "only the parent tests invoke this with an allocator guard"]
fn guarded_rejection_probe() {
    let case = std::env::var("VRMANAGER_APK_GUARD_CASE").expect("run through guarded_child");
    let input = manifest("com.example.review");
    let apk = match case.as_str() {
        "declared" => Some(ApkFixture::new(&input, 0, Some(1 << 30))),
        "actual" => Some(ApkFixture::new(
            &input,
            20 * 1024 * 1024,
            Some(input.len() as u32),
        )),
        "compressed" => Some(ApkFixture::new(&input, 20 * 1024 * 1024, None)),
        "pool-count" => None,
        _ if case.starts_with("axml-") => None,
        _ => panic!("unknown probe {case}"),
    };
    let axml = if case.starts_with("axml-") {
        budget_case(&case)
    } else {
        enormous_pool_count()
    };
    ALLOCATION_LIMIT.store(8 * 1024 * 1024, Ordering::Relaxed);
    TOTAL_ALLOCATION_BUDGET.store(32 * 1024 * 1024, Ordering::Relaxed);
    if let Some(apk) = apk.as_ref() {
        let result = inspect_apk(&apk.0);
        ALLOCATION_LIMIT.store(usize::MAX, Ordering::Relaxed);
        TOTAL_ALLOCATION_BUDGET.store(usize::MAX, Ordering::Relaxed);
        assert!(
            matches!(&result, Err(CoreError::Apk(message)) if message.contains("limit")),
            "{result:?}"
        );
    } else {
        let result = axml::parse(&axml);
        ALLOCATION_LIMIT.store(usize::MAX, Ordering::Relaxed);
        TOTAL_ALLOCATION_BUDGET.store(usize::MAX, Ordering::Relaxed);
        assert!(
            result.is_none(),
            "malformed or oversized document must fail closed"
        );
    }
}

#[test]
fn declared_manifest_size_is_bounded_before_reserve() {
    guarded_child("declared");
}

#[test]
fn actual_manifest_read_ignores_forged_small_declared_size() {
    guarded_child("actual");
}

#[test]
fn compressed_manifest_size_is_bounded_before_read() {
    guarded_child("compressed");
}

#[test]
fn valid_apk_metadata_is_unchanged() {
    let apk = ApkFixture::new(&manifest("com.example.review"), 0, None);
    let meta = inspect_apk(&apk.0).expect("valid binary-XML APK");
    assert_eq!(meta.package, "com.example.review");
    assert_eq!(meta.size_bytes, std::fs::metadata(&apk.0).unwrap().len());
    let json = serde_json::to_value(meta).unwrap();
    let mut keys: Vec<_> = json
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "abis",
            "applicationLabel",
            "fileName",
            "hasIcon",
            "minSdk",
            "package",
            "path",
            "permissions",
            "sizeBytes",
            "targetSdk",
            "versionCode",
            "versionName",
            "vrCapable"
        ]
    );
}
