use super::{fixture::*, inspect_apk, CoreError};

#[test]
fn malformed_package_identifiers_are_rejected_not_normalized() {
    let invalid = [
        "com.example; printf APK_MARKER; #",
        " com.example",
        "com.example ",
        "com..example",
        ".com.example",
        "com.example.",
        "1com.example",
        "com.1example",
        "com._example",
        "com.example-app",
        "com.exámple",
        "com.example\n",
        "com.example\0",
        "com/example",
        "-com.example",
        "com.example$(id)",
        "com.example'quoted",
        "@0x7f000001",
    ];
    let accepted: Vec<_> = invalid
        .into_iter()
        .filter(|package| {
            let apk = ApkFixture::new(&manifest(package), 0, None);
            inspect_apk(&apk.0).is_ok()
        })
        .collect();
    assert!(
        accepted.is_empty(),
        "invalid packages accepted: {accepted:?}"
    );
}

#[test]
fn package_length_is_bounded_without_truncation() {
    let package = format!("com.{}", "a".repeat(256 - "com.".len()));
    let apk = ApkFixture::new(&manifest(&package), 0, None);
    assert!(matches!(inspect_apk(&apk.0), Err(CoreError::Apk(_))));
}

#[test]
fn valid_identifier_case_digits_and_underscore_are_preserved() {
    let longest = format!("com.{}", "a".repeat(255 - "com.".len()));
    for package in ["com.Example.VR_app2", "android", longest.as_str()] {
        let apk = ApkFixture::new(&manifest(package), 0, None);
        assert_eq!(inspect_apk(&apk.0).unwrap().package, package);
    }
}

#[test]
fn real_binary_values_keep_metadata_semantics() {
    let bytes = document(&[
        pool(
            &[
                "manifest",
                "package",
                "com.example.VR",
                "versionCode",
                "versionName",
                "1.2.3",
                "uses-sdk",
                "minSdkVersion",
                "targetSdkVersion",
                "application",
                "label",
                "VR-пример 🥽",
                "uses-permission",
                "name",
                "android.permission.INTERNET",
                "uses-feature",
                "android.hardware.vr.headtracking",
            ],
            true,
        ),
        element(0, &[(1, 2, 3, 2), (3, u32::MAX, 0x10, 123), (4, 5, 3, 5)]),
        element(6, &[(7, u32::MAX, 0x10, 29), (8, u32::MAX, 0x10, 32)]),
        end_element(6),
        element(9, &[(10, 11, 3, 11)]),
        end_element(9),
        element(12, &[(13, 14, 3, 14)]),
        end_element(12),
        element(15, &[(13, 16, 3, 16)]),
        end_element(15),
        end_element(0),
    ]);
    let apk = ApkFixture::new(&bytes, 0, None);
    let meta = inspect_apk(&apk.0).unwrap();
    assert_eq!(meta.package, "com.example.VR");
    assert_eq!(meta.version_name.as_deref(), Some("1.2.3"));
    assert_eq!(meta.version_code, Some(123));
    assert_eq!(meta.min_sdk, Some(29));
    assert_eq!(meta.target_sdk, Some(32));
    assert_eq!(meta.application_label.as_deref(), Some("VR-пример 🥽"));
    assert_eq!(meta.permissions, ["android.permission.INTERNET"]);
    assert!(meta.vr_capable);
}
