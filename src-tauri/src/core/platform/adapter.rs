//! Platform adapters.
//!
//! One trait, three implementations. Everything platform-specific (vendor
//! launch intents, which capabilities exist, how to read battery) lives in an
//! adapter, so adding a future XR platform means adding a file here — not
//! editing branches scattered across the app.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use crate::core::adb::client::AdbClient;
use crate::core::cancel::CancelToken;
use crate::core::platform::capability::{Capabilities, Capability};
use crate::core::platform::detect::Platform;
use crate::error::CoreResult;

/// Boxed future alias: keeps [`PlatformAdapter`] usable as `dyn` (an
/// `async fn` in a trait is not dyn compatible).
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Behaviour that differs between headset vendors.
pub trait PlatformAdapter: Send + Sync {
    fn platform(&self) -> Platform;

    /// Human-readable device name from vendor properties, best effort.
    fn display_model(&self, props: &HashMap<String, String>) -> String;

    /// Capabilities derivable from properties alone (no extra round trips).
    fn capabilities(&self, props: &HashMap<String, String>) -> Capabilities;

    /// Packages that should be hidden from the user's Library by default
    /// (vendor system apps that are not user-facing).
    fn is_system_noise(&self, package: &str) -> bool {
        const COMMON: &[&str] = &[
            "com.android.",
            "com.google.android.",
            "com.qualcomm.",
            "android.auto_generated",
        ];
        COMMON.iter().any(|p| package.starts_with(p))
    }

    /// Launch by package using the platform's `monkey` command through the
    /// shared literal-argument shell boundary.
    fn launch<'a>(
        &'a self,
        client: &'a AdbClient,
        serial: &'a str,
        package: &'a str,
        cancel: &'a CancelToken,
    ) -> BoxFuture<'a, CoreResult<()>> {
        Box::pin(async move {
            if package.len() > 255
                || package.split('.').any(|part| {
                    !part.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
                        || !part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                })
            {
                return Err(crate::error::CoreError::other(
                    "Invalid Android package name.",
                ));
            }
            // Use the shared literal-argv boundary: ADB reparses shell input.
            let args = [
                "monkey",
                "-p",
                package,
                "-c",
                "android.intent.category.LAUNCHER",
                "1",
            ];
            client.shell(serial, &args, cancel).await?;
            Ok(())
        })
    }
}

pub struct PicoAdapter;
pub struct QuestAdapter;
pub struct AndroidAdapter;

fn prop<'a>(props: &'a HashMap<String, String>, key: &str) -> &'a str {
    props.get(key).map(String::as_str).unwrap_or_default()
}

/// Shared capability derivation: baseline plus property-driven extras.
fn base_caps(props: &HashMap<String, String>) -> Capabilities {
    let mut caps = Capabilities::baseline_android();

    // `cmd package install-incremental`/streaming install needs a recent
    // platform; SDK 30+ is the reliable floor.
    if prop(props, "ro.build.version.sdk")
        .parse::<u32>()
        .map(|sdk| sdk >= 30)
        .unwrap_or(false)
    {
        caps.insert(Capability::StreamingInstall);
    }

    caps
}

impl PlatformAdapter for PicoAdapter {
    fn platform(&self) -> Platform {
        Platform::Pico
    }

    fn display_model(&self, props: &HashMap<String, String>) -> String {
        let model = prop(props, "ro.product.model");
        if !model.is_empty() && model.to_ascii_lowercase().contains("pico") {
            return model.replace('_', " ");
        }
        // Map known Pico codenames when the model field is useless.
        let device = prop(props, "ro.product.device").to_ascii_lowercase();
        let mapped = match device.as_str() {
            "phoenix" => Some("Pico 4"),
            "merlin" => Some("Pico 4 Ultra"),
            "jaguar" => Some("Pico Neo 3"),
            _ => None,
        };
        match mapped {
            Some(name) => name.to_string(),
            None if !model.is_empty() => format!("Pico ({})", model.replace('_', " ")),
            None => "Pico headset".to_string(),
        }
    }

    fn capabilities(&self, props: &HashMap<String, String>) -> Capabilities {
        let mut caps = base_caps(props);
        caps.insert(Capability::TemperatureInfo);
        caps
    }

    fn is_system_noise(&self, package: &str) -> bool {
        const PICO: &[&str] = &["com.pico.", "com.pvr.", "com.picovr.", "com.bytedance."];
        PICO.iter().any(|p| package.starts_with(p))
            || package.starts_with("com.android.")
            || package.starts_with("com.google.android.")
            || package.starts_with("com.qualcomm.")
    }
}

impl PlatformAdapter for QuestAdapter {
    fn platform(&self) -> Platform {
        Platform::Quest
    }

    fn display_model(&self, props: &HashMap<String, String>) -> String {
        let model = prop(props, "ro.product.model");
        if !model.is_empty() && model.to_ascii_lowercase().contains("quest") {
            return model.replace('_', " ");
        }
        let device = prop(props, "ro.product.device").to_ascii_lowercase();
        let mapped = match device.as_str() {
            "monterey" => Some("Quest 1"),
            "hollywood" => Some("Quest 2"),
            "seacliff" => Some("Quest Pro"),
            "eureka" => Some("Quest 3"),
            "panther" => Some("Quest 3S"),
            _ => None,
        };
        match mapped {
            Some(name) => name.to_string(),
            None if !model.is_empty() => format!("Quest ({})", model.replace('_', " ")),
            None => "Meta Quest".to_string(),
        }
    }

    fn capabilities(&self, props: &HashMap<String, String>) -> Capabilities {
        base_caps(props)
    }

    fn is_system_noise(&self, package: &str) -> bool {
        const META: &[&str] = &["com.oculus.", "com.meta.", "com.facebook.", "com.qualcomm."];
        META.iter().any(|p| package.starts_with(p))
            || package.starts_with("com.android.")
            || package.starts_with("com.google.android.")
    }
}

impl PlatformAdapter for AndroidAdapter {
    fn platform(&self) -> Platform {
        Platform::Android
    }

    fn display_model(&self, props: &HashMap<String, String>) -> String {
        let brand = prop(props, "ro.product.manufacturer");
        let model = prop(props, "ro.product.model");
        match (brand.is_empty(), model.is_empty()) {
            (false, false) => {
                if model
                    .to_ascii_lowercase()
                    .starts_with(&brand.to_ascii_lowercase())
                {
                    model.to_string()
                } else {
                    format!("{brand} {model}")
                }
            }
            (true, false) => model.to_string(),
            (false, true) => brand.to_string(),
            (true, true) => "Android device".to_string(),
        }
    }

    fn capabilities(&self, props: &HashMap<String, String>) -> Capabilities {
        base_caps(props)
    }
}

/// Pick the adapter for a detected platform. `Unknown` degrades to the
/// generic Android adapter so the user still gets basic management.
pub fn adapter_for(platform: Platform) -> Box<dyn PlatformAdapter> {
    match platform {
        Platform::Pico => Box::new(PicoAdapter),
        Platform::Quest => Box::new(QuestAdapter),
        Platform::Android | Platform::Unknown => Box::new(AndroidAdapter),
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
    fn pico_codename_maps_to_marketing_name() {
        let p = props(&[
            ("ro.product.device", "phoenix"),
            ("ro.product.model", "A8110"),
        ]);
        assert_eq!(PicoAdapter.display_model(&p), "Pico 4");
    }

    #[test]
    fn quest_codename_maps_to_marketing_name() {
        let p = props(&[("ro.product.device", "eureka")]);
        assert_eq!(QuestAdapter.display_model(&p), "Quest 3");
    }

    #[test]
    fn android_name_is_not_duplicated() {
        let p = props(&[
            ("ro.product.manufacturer", "Xiaomi"),
            ("ro.product.model", "Xiaomi Pad 6"),
        ]);
        assert_eq!(AndroidAdapter.display_model(&p), "Xiaomi Pad 6");

        let p = props(&[
            ("ro.product.manufacturer", "Google"),
            ("ro.product.model", "Pixel 8"),
        ]);
        assert_eq!(AndroidAdapter.display_model(&p), "Google Pixel 8");
    }

    #[test]
    fn streaming_install_requires_modern_sdk() {
        let old = props(&[("ro.build.version.sdk", "29")]);
        assert!(!PicoAdapter
            .capabilities(&old)
            .has(Capability::StreamingInstall));

        let new = props(&[("ro.build.version.sdk", "32")]);
        assert!(QuestAdapter
            .capabilities(&new)
            .has(Capability::StreamingInstall));
    }

    #[test]
    fn vendor_packages_are_filtered_per_platform() {
        assert!(PicoAdapter.is_system_noise("com.pvr.launcher"));
        assert!(!PicoAdapter.is_system_noise("com.beatgames.beatsaber"));
        assert!(QuestAdapter.is_system_noise("com.oculus.shellenv"));
        assert!(!QuestAdapter.is_system_noise("com.vrchat.vrchat"));
    }

    #[test]
    fn unknown_platform_still_gets_a_working_adapter() {
        let adapter = adapter_for(Platform::Unknown);
        assert_eq!(adapter.platform(), Platform::Android);
        assert!(adapter
            .capabilities(&props(&[("ro.build.version.sdk", "33")]))
            .has(Capability::InstallApk));
    }
}
