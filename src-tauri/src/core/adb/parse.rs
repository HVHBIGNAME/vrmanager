//! Pure parsers for ADB text output.
//!
//! Kept free of IO on purpose: these are the parts most likely to break when a
//! vendor ships an odd build, so they must be unit-testable without hardware.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AdbDeviceState {
    Device,
    Unauthorized,
    Offline,
    Connecting,
    Recovery,
    Sideload,
    Bootloader,
    Rescue,
    NoPermissions,
    Unknown,
}

impl AdbDeviceState {
    pub fn parse(raw: &str) -> Self {
        match raw.trim() {
            "device" => Self::Device,
            "unauthorized" => Self::Unauthorized,
            "offline" => Self::Offline,
            "connecting" | "authorizing" => Self::Connecting,
            "recovery" => Self::Recovery,
            "sideload" => Self::Sideload,
            "bootloader" | "fastboot" => Self::Bootloader,
            "rescue" => Self::Rescue,
            s if s.starts_with("no permissions") => Self::NoPermissions,
            _ => Self::Unknown,
        }
    }

    /// Can we run shell commands right now?
    pub fn is_ready(self) -> bool {
        matches!(self, Self::Device)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Device => "device",
            Self::Unauthorized => "unauthorized",
            Self::Offline => "offline",
            Self::Connecting => "connecting",
            Self::Recovery => "recovery",
            Self::Sideload => "sideload",
            Self::Bootloader => "bootloader",
            Self::Rescue => "rescue",
            Self::NoPermissions => "no permissions",
            Self::Unknown => "unknown",
        }
    }
}

/// One line of `adb devices -l`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdbDeviceEntry {
    pub serial: String,
    pub state: AdbDeviceState,
    /// Extra `key:value` pairs adb appends in long mode (model, product, ...).
    pub properties: HashMap<String, String>,
}

impl AdbDeviceEntry {
    /// True when the serial looks like `host:port`, i.e. a network transport.
    pub fn is_network(&self) -> bool {
        is_network_serial(&self.serial)
    }

    pub fn model_hint(&self) -> Option<&str> {
        self.properties.get("model").map(String::as_str)
    }
}

/// `host:port` detection that tolerates IPv6 (`[::1]:5555`) and hostnames.
pub fn is_network_serial(serial: &str) -> bool {
    match serial.rsplit_once(':') {
        Some((host, port)) => {
            !host.is_empty() && !port.is_empty() && port.chars().all(|c| c.is_ascii_digit())
        }
        None => false,
    }
}

/// Parse `adb devices -l` output.
///
/// Tolerates the header line, blank lines, daemon chatter and long-format
/// trailing attributes.
pub fn parse_devices(raw: &str) -> Vec<AdbDeviceEntry> {
    let mut out = Vec::new();

    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty()
            || line.starts_with("List of devices")
            || line.starts_with('*')
            || line.starts_with("adb server")
            || line.starts_with("daemon ")
        {
            continue;
        }

        let mut parts = line.split_whitespace();
        let Some(serial) = parts.next() else { continue };

        // `no permissions` is two words; rebuild the state from the remainder
        // until the first `key:value` attribute.
        let mut state_words: Vec<&str> = Vec::new();
        let mut properties = HashMap::new();
        for token in parts {
            if let Some((key, value)) = token.split_once(':') {
                if !key.is_empty() && !state_words.is_empty() {
                    properties.insert(key.to_string(), value.to_string());
                    continue;
                }
            }
            state_words.push(token);
        }

        if state_words.is_empty() {
            continue;
        }

        out.push(AdbDeviceEntry {
            serial: serial.to_string(),
            state: AdbDeviceState::parse(&state_words.join(" ")),
            properties,
        });
    }

    out
}

/// Parse the output of `adb shell getprop` (the full dump form:
/// `[key]: [value]` per line).
pub fn parse_getprop(raw: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for line in raw.lines() {
        let line = line.trim();
        if !line.starts_with('[') {
            continue;
        }
        let Some((key_part, value_part)) = line.split_once("]:") else {
            continue;
        };
        let key = key_part.trim_start_matches('[').trim();
        let value = value_part
            .trim()
            .trim_start_matches('[')
            .trim_end_matches(']');
        if !key.is_empty() {
            out.insert(key.to_string(), value.to_string());
        }
    }
    out
}

/// Pull an `INSTALL_FAILED_*` / `Failure [...]` code out of pm/adb output.
pub fn parse_install_failure(raw: &str) -> Option<String> {
    for token in raw.split(|c: char| c.is_whitespace() || c == '[' || c == ']' || c == ':') {
        let t = token.trim();
        if t.starts_with("INSTALL_FAILED") || t.starts_with("INSTALL_PARSE_FAILED") {
            return Some(t.to_string());
        }
    }
    if raw.to_ascii_lowercase().contains("failure") {
        return Some("INSTALL_FAILED_UNKNOWN".to_string());
    }
    None
}

/// Parse `pm list packages -3 -f` style output into (package, apk_path).
pub fn parse_package_list(raw: &str) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("package:") else {
            continue;
        };
        // With `-f`: `package:/data/app/.../base.apk=com.example.app`
        match rest.rsplit_once('=') {
            Some((path, pkg)) if !pkg.is_empty() => {
                out.push((pkg.to_string(), Some(path.to_string())))
            }
            _ => {
                if !rest.is_empty() {
                    out.push((rest.to_string(), None))
                }
            }
        }
    }
    out
}

/// Extract the first IPv4 address from `ip route`/`ip addr` style output.
pub fn parse_first_ipv4(raw: &str) -> Option<String> {
    fn looks_like_ipv4(token: &str) -> bool {
        let octets: Vec<&str> = token.split('.').collect();
        octets.len() == 4
            && octets
                .iter()
                .all(|o| !o.is_empty() && o.len() <= 3 && o.chars().all(|c| c.is_ascii_digit()))
            && octets
                .iter()
                .all(|o| o.parse::<u16>().map(|v| v <= 255).unwrap_or(false))
    }

    for line in raw.lines() {
        // `ip route get 1.1.1.1` → "... src 192.168.1.42 ..."
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if let Some(pos) = tokens.iter().position(|t| *t == "src") {
            if let Some(candidate) = tokens.get(pos + 1) {
                if looks_like_ipv4(candidate) && !candidate.starts_with("127.") {
                    return Some((*candidate).to_string());
                }
            }
        }
        // `ip -f inet addr show wlan0` → "inet 192.168.1.42/24 ..."
        if let Some(pos) = tokens.iter().position(|t| *t == "inet") {
            if let Some(candidate) = tokens.get(pos + 1) {
                let ip = candidate.split('/').next().unwrap_or_default();
                if looks_like_ipv4(ip) && !ip.starts_with("127.") {
                    return Some(ip.to_string());
                }
            }
        }
        // bare `getprop dhcp.wlan0.ipaddress`
        let bare = line.trim();
        if looks_like_ipv4(bare) && !bare.starts_with("127.") {
            return Some(bare.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mixed_device_list() {
        let raw = "\
* daemon not running; starting now at tcp:5037
* daemon started successfully
List of devices attached
PA7B10K2        device product:phoenix model:Pico_4 device:phoenix transport_id:3
192.168.1.44:5555       device product:hollywood model:Quest_3 transport_id:5
ZY3270XYZ       unauthorized
04f1a2b3        no permissions; see [http://developer.android.com/tools/device.html]
";
        let devices = parse_devices(raw);
        assert_eq!(devices.len(), 4, "got: {devices:#?}");

        assert_eq!(devices[0].serial, "PA7B10K2");
        assert_eq!(devices[0].state, AdbDeviceState::Device);
        assert_eq!(devices[0].model_hint(), Some("Pico_4"));
        assert!(!devices[0].is_network());

        assert!(devices[1].is_network());
        assert_eq!(devices[1].model_hint(), Some("Quest_3"));

        assert_eq!(devices[2].state, AdbDeviceState::Unauthorized);
        assert_eq!(devices[3].state, AdbDeviceState::NoPermissions);
    }

    #[test]
    fn empty_list_is_empty_not_garbage() {
        assert!(parse_devices("List of devices attached\n\n").is_empty());
    }

    #[test]
    fn network_serial_detection_handles_ipv6_and_hostnames() {
        assert!(is_network_serial("192.168.1.10:5555"));
        assert!(is_network_serial("[fe80::1]:5555"));
        assert!(is_network_serial("pico.local:5555"));
        assert!(!is_network_serial("PA7B10K2"));
        assert!(!is_network_serial("weird:port"));
    }

    #[test]
    fn parses_getprop_dump() {
        let raw = "\
[ro.product.manufacturer]: [Pico]
[ro.product.model]: [Pico 4]
[ro.build.version.release]: [10]
[ro.build.version.sdk]: [29]
[empty.prop]: []
";
        let props = parse_getprop(raw);
        assert_eq!(props.get("ro.product.manufacturer").unwrap(), "Pico");
        assert_eq!(props.get("ro.product.model").unwrap(), "Pico 4");
        assert_eq!(props.get("ro.build.version.sdk").unwrap(), "29");
        assert_eq!(props.get("empty.prop").unwrap(), "");
    }

    #[test]
    fn finds_install_failure_codes() {
        assert_eq!(
            parse_install_failure("Failure [INSTALL_FAILED_INSUFFICIENT_STORAGE]").as_deref(),
            Some("INSTALL_FAILED_INSUFFICIENT_STORAGE")
        );
        assert_eq!(
            parse_install_failure(
                "adb: failed to install x.apk: Failure [INSTALL_PARSE_FAILED_NO_CERTIFICATES: ...]"
            )
            .as_deref(),
            Some("INSTALL_PARSE_FAILED_NO_CERTIFICATES")
        );
        assert_eq!(parse_install_failure("Success").as_deref(), None);
    }

    #[test]
    fn parses_package_list_with_and_without_paths() {
        let raw = "\
package:/data/app/~~abc==/com.beatgames.beatsaber-1/base.apk=com.beatgames.beatsaber
package:com.pico.browser
";
        let pkgs = parse_package_list(raw);
        assert_eq!(pkgs.len(), 2);
        assert_eq!(pkgs[0].0, "com.beatgames.beatsaber");
        assert!(pkgs[0].1.as_deref().unwrap().ends_with("base.apk"));
        assert_eq!(pkgs[1].0, "com.pico.browser");
        assert!(pkgs[1].1.is_none());
    }

    #[test]
    fn extracts_device_ip() {
        assert_eq!(
            parse_first_ipv4("1.1.1.1 via 192.168.1.1 dev wlan0 src 192.168.1.44 uid 0").as_deref(),
            Some("192.168.1.44")
        );
        assert_eq!(
            parse_first_ipv4("inet 127.0.0.1/8 scope host lo\ninet 10.0.0.7/24 brd ... wlan0")
                .as_deref(),
            Some("10.0.0.7")
        );
        assert_eq!(parse_first_ipv4("no addresses here"), None);
    }
}
