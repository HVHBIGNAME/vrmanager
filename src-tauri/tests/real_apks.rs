use std::path::Path;
use vrmanager_lib::core::apk::inspect_apk;

#[test]
fn parses_real_world_apks() {
    let candidates = [
        r"C:\Users\Pisun4ik\Desktop\PICO_Crashland_2.007_OVRport_3.4.2\com.LACGames.Crashland.fixed.apk",
        r"C:\Users\Pisun4ik\Downloads\AyuGram Desktop\PICO_Steam_Link_2.0.22(5002252).apk",
        r"C:\Users\Pisun4ik\AppData\Local\Programs\SideQuest\resources\quest-mic-relay.apk",
    ];
    let mut checked = 0;
    for c in candidates {
        let p = Path::new(c);
        if !p.exists() {
            eprintln!("SKIP (absent): {c}");
            continue;
        }
        match inspect_apk(p) {
            Ok(m) => {
                checked += 1;
                println!("--------------------------------------------------");
                println!("file      : {}", m.file_name);
                println!("package   : {}", m.package);
                println!("name      : {}", m.display_name());
                println!(
                    "version   : {:?} (code {:?})",
                    m.version_name, m.version_code
                );
                println!("sdk       : min {:?} target {:?}", m.min_sdk, m.target_sdk);
                println!("abis      : {:?}", m.abis);
                println!("size      : {:.1} MB", m.size_mb());
                println!("perms     : {} entries", m.permissions.len());
                println!("vr_capable: {}", m.vr_capable);
                assert!(!m.package.is_empty(), "package must be parsed");
                assert!(
                    m.package.contains('.'),
                    "package looks wrong: {}",
                    m.package
                );
            }
            Err(e) => panic!("FAILED to parse real APK {c}: {e}"),
        }
    }
    assert!(checked > 0, "no real APKs were available to test");
    println!("\nparsed {checked} real APK(s) successfully");
}
