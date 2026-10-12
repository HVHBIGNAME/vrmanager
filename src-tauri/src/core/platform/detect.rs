//! Platform detection.
//!
//! Deliberately evidence-based: a single property is never enough, because
//! vendors ship inconsistent builds (Pico devices that report `Pico` in
//! `ro.product.brand` but a codename in `ro.product.model`, Quest units that
//! only identify themselves through `ro.product.vr.*`, etc).
//!
//! Each signal contributes a weight; the best score above a floor wins,
//! otherwise the platform is [`Platform::Unknown`] and the UI degrades to a
//! generic Android device instead of lying.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Platform {
    Pico,
    Quest,
    Android,
    Unknown,
}

impl Platform {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pico => "Pico",
            Self::Quest => "Meta Quest",
            Self::Android => "Android",
            Self::Unknown => "Unknown",
        }
    }

    /// Is this a head-mounted display (affects copy and catalog filters)?
    pub fn is_headset(self) -> bool {
        matches!(self, Self::Pico | Self::Quest)
    }
}

/// Properties worth fetching when a full `getprop` dump is too expensive.
pub const DETECTION_PROPS: &[&str] = &[
    "ro.product.manufacturer",
    "ro.product.brand",
    "ro.product.model",
    "ro.product.device",
    "ro.product.name",
    "ro.build.fingerprint",
    "ro.build.characteristics",
    "ro.pvr.sdk.version",
    "ro.pui.version",
    "ro.vros.build.version",
    "ro.oculus.sdk.version",
    "ro.product.vr.model",
];

fn get<'a>(props: &'a HashMap<String, String>, key: &str) -> &'a str {
    props.get(key).map(String::as_str).unwrap_or_default()
}

/// Score each platform from the available evidence and return the winner.
pub fn detect_platform(props: &HashMap<String, String>) -> Platform {
    let mut pico = 0i32;
    let mut quest = 0i32;

    let manufacturer = get(props, "ro.product.manufacturer").to_ascii_lowercase();
    let brand = get(props, "ro.product.brand").to_ascii_lowercase();
    let model = get(props, "ro.product.model").to_ascii_lowercase();
    let device = get(props, "ro.product.device").to_ascii_lowercase();
    let name = get(props, "ro.product.name").to_ascii_lowercase();
    let fingerprint = get(props, "ro.build.fingerprint").to_ascii_lowercase();

    // --- Pico signals (Pico / ByteDance) ---
    for field in [&manufacturer, &brand] {
        if field.contains("pico") || field.contains("bytedance") {
            pico += 4;
        }
    }
    if model.contains("pico") {
        pico += 3;
    }
    if name.contains("pico") || device.contains("pico") {
        pico += 2;
    }
    if fingerprint.contains("pico") || fingerprint.contains("bytedance") {
        pico += 2;
    }
    // Pico-only vendor property namespaces.
    for key in ["ro.pvr.sdk.version", "ro.pui.version"] {
        if !get(props, key).is_empty() {
            pico += 5;
        }
    }
    if props.keys().any(|k| k.starts_with("ro.pvr.")) {
        pico += 3;
    }

    // --- Quest signals (Meta / Oculus) ---
    for field in [&manufacturer, &brand] {
        if field.contains("oculus") || field.contains("meta") || field.contains("facebook") {
            quest += 4;
        }
    }
    if model.contains("quest") {
        quest += 3;
    }
    // Known Quest board/codenames.
    for codename in ["monterey", "hollywood", "seacliff", "eureka", "panther"] {
        if device.contains(codename) || name.contains(codename) {
            quest += 3;
        }
    }
    if fingerprint.contains("oculus") || fingerprint.contains("quest") {
        quest += 2;
    }
    for key in ["ro.vros.build.version", "ro.oculus.sdk.version"] {
        if !get(props, key).is_empty() {
            quest += 5;
        }
    }
    if props.keys().any(|k| k.starts_with("ro.oculus.")) {
        quest += 3;
    }

    const FLOOR: i32 = 4;
    if pico >= FLOOR && pico >= quest {
        return Platform::Pico;
    }
    if quest >= FLOOR {
        return Platform::Quest;
    }

    // Not a known headset: is it at least a plausible Android device?
    let looks_android = !get(props, "ro.build.version.sdk").is_empty()
        || !get(props, "ro.build.version.release").is_empty()
        || !manufacturer.is_empty() && !model.is_empty();

    // A VR characteristic without a vendor match still should not be called a phone.
    let vr_hint = get(props, "ro.build.characteristics").contains("vr")
        || !get(props, "ro.product.vr.model").is_empty();

    // A VR characteristic without a vendor match still should not be called a
    // phone. Anything else with Android signals is a plausible phone; no
    // signal at all stays an honest `Unknown` (never a guess).
    if looks_android && !vr_hint {
        Platform::Android
    } else {
        Platform::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn props(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn detects_pico_4() {
        let p = props(&[
            ("ro.product.manufacturer", "Pico"),
            ("ro.product.model", "Pico 4"),
            ("ro.product.device", "phoenix"),
            ("ro.pvr.sdk.version", "2.5.0"),
            ("ro.build.version.sdk", "29"),
        ]);
        assert_eq!(detect_platform(&p), Platform::Pico);
    }

    #[test]
    fn detects_pico_from_vendor_props_only() {
        // Model is a codename, manufacturer is generic — vendor namespace saves us.
        let p = props(&[
            ("ro.product.manufacturer", "alps"),
            ("ro.product.model", "A8110"),
            ("ro.pui.version", "5.4.0"),
            ("ro.build.version.sdk", "29"),
        ]);
        assert_eq!(detect_platform(&p), Platform::Pico);
    }

    #[test]
    fn detects_quest_3() {
        let p = props(&[
            ("ro.product.manufacturer", "Oculus"),
            ("ro.product.model", "Quest 3"),
            ("ro.product.device", "eureka"),
            ("ro.build.version.sdk", "32"),
        ]);
        assert_eq!(detect_platform(&p), Platform::Quest);
    }

    #[test]
    fn detects_quest_by_codename_and_vros() {
        let p = props(&[
            ("ro.product.manufacturer", "Meta"),
            ("ro.product.device", "hollywood"),
            ("ro.vros.build.version", "63.0"),
            ("ro.build.version.sdk", "32"),
        ]);
        assert_eq!(detect_platform(&p), Platform::Quest);
    }

    #[test]
    fn plain_phone_is_android_not_a_headset() {
        let p = props(&[
            ("ro.product.manufacturer", "Xiaomi"),
            ("ro.product.model", "Redmi Note 9 Pro"),
            ("ro.build.version.sdk", "30"),
        ]);
        assert_eq!(detect_platform(&p), Platform::Android);
    }

    #[test]
    fn empty_props_are_unknown_never_a_guess() {
        assert_eq!(detect_platform(&HashMap::new()), Platform::Unknown);
    }

    #[test]
    fn unknown_vr_device_does_not_masquerade_as_a_phone() {
        let p = props(&[
            ("ro.product.manufacturer", "SomeOem"),
            ("ro.product.model", "XR-1"),
            ("ro.build.characteristics", "vr,nosdcard"),
            ("ro.build.version.sdk", "31"),
        ]);
        assert_eq!(detect_platform(&p), Platform::Unknown);
    }

    #[test]
    fn single_weak_signal_does_not_win() {
        // The word "pico" inside an unrelated fingerprint alone is not enough.
        let p = props(&[("ro.build.fingerprint", "acme/picobello/x:11/RQ/1:user")]);
        assert_ne!(detect_platform(&p), Platform::Pico);
    }
}
