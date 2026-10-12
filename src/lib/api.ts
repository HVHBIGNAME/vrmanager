/**
 * Typed bridge to the Rust core.
 *
 * Every Tauri command is wrapped here so screens never touch `invoke`
 * directly and every error arrives as a `UiError` with a human message.
 */
import { invoke } from "@tauri-apps/api/core";
import type { InstallJobSnapshot } from "./events";

// ---- error model -----------------------------------------------------------

export interface UiError {
  kind: string;
  message: string;
  details?: string;
  retryable: boolean;
}

export function isUiError(value: unknown): value is UiError {
  return (
    typeof value === "object" &&
    value !== null &&
    "message" in value &&
    "kind" in value
  );
}

/** Normalise anything thrown by `invoke` into a UiError. */
export function toUiError(err: unknown): UiError {
  if (isUiError(err)) return err;
  return {
    kind: "unknown",
    message: typeof err === "string" ? err : "Something went wrong.",
    details: err instanceof Error ? err.stack : undefined,
    retryable: true,
  };
}

// ---- domain types (mirror the Rust serde shapes) ---------------------------

export type Platform = "pico" | "quest" | "android" | "unknown";
export type ConnectionType = "usb" | "wireless";
export type DeviceState =
  | "device"
  | "unauthorized"
  | "offline"
  | "connecting"
  | "recovery"
  | "sideload"
  | "bootloader"
  | "rescue"
  | "noPermissions"
  | "unknown";

export interface DeviceSnapshot {
  serial: string;
  displayName: string;
  modelName: string;
  platform: Platform;
  connection: ConnectionType;
  state: DeviceState;
  ready: boolean;
  batteryPercent?: number | null;
  storageFreeBytes?: number | null;
}

export interface DeviceInfo {
  manufacturer: string;
  model: string;
  deviceCodename: string;
  androidVersion: string;
  sdkVersion?: number | null;
  buildFingerprint: string;
  abi: string;
  supportedAbis: string[];
  batteryPercent?: number | null;
  batteryCharging?: boolean | null;
  temperatureCelsius?: number | null;
  storageTotalBytes?: number | null;
  storageFreeBytes?: number | null;
  ramTotalBytes?: number | null;
  ipAddress?: string | null;
}

export type Capability =
  | "installApk"
  | "uninstallApk"
  | "launchApp"
  | "forceStop"
  | "clearData"
  | "clearCache"
  | "exportApk"
  | "listApps"
  | "wirelessAdb"
  | "streamingInstall"
  | "screenCapture"
  | "batteryInfo"
  | "storageInfo"
  | "temperatureInfo";

export interface Device {
  serial: string;
  customName?: string | null;
  modelName: string;
  platform: Platform;
  connection: ConnectionType;
  state: DeviceState;
  info: DeviceInfo;
  capabilities: Capability[];
  wirelessAddress?: string | null;
}

export interface InstalledApp {
  package: string;
  apkPath?: string | null;
  versionName?: string | null;
  versionCode?: number | null;
  system: boolean;
}

export interface ApkMetadata {
  path: string;
  fileName: string;
  sizeBytes: number;
  package: string;
  applicationLabel?: string | null;
  versionName?: string | null;
  versionCode?: number | null;
  minSdk?: number | null;
  targetSdk?: number | null;
  abis: string[];
  permissions: string[];
  hasIcon: boolean;
  vrCapable: boolean;
}

export interface Environment {
  adbAvailable: boolean;
  adbPath?: string | null;
  adbSource?: "configured" | "bundled" | "systemPath" | "androidSdk" | null;
  adbVersion?: string | null;
  backendConfigured: boolean;
  onboarded: boolean;
}

export interface Settings {
  adbPath?: string | null;
  backendUrl?: string | null;
  showSystemApps: boolean;
  selectedSerials: string[];
  onboarded: boolean;
}

export interface ApkInspectionBatch {
  apks: ApkMetadata[];
  errors: { path: string; error: UiError }[];
}

export interface InstallJob {
  jobId: string;
}

// ---- command wrappers ------------------------------------------------------

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (err) {
    throw toUiError(err);
  }
}

export const api = {
  // environment & settings
  getEnvironment: () => call<Environment>("get_environment"),
  getSettings: () => call<Settings>("get_settings"),
  setAdbPath: (path: string | null) => call<Settings>("set_adb_path", { path }),
  setShowSystemApps: (show: boolean) =>
    call<Settings>("set_show_system_apps", { show }),
  setSelectedDevices: (serials: string[]) =>
    call<Settings>("set_selected_devices", { serials }),
  completeOnboarding: () => call<Settings>("complete_onboarding"),

  // devices
  listDevices: () => call<DeviceSnapshot[]>("list_devices"),
  getDevice: (serial: string) => call<Device>("get_device", { serial }),
  renameDevice: (serial: string, name: string) =>
    call<void>("rename_device", { serial, name }),
  deviceCapabilities: (serial: string) =>
    call<string[]>("device_capabilities", { serial }),
  enableWireless: (serial: string) => call<string>("enable_wireless", { serial }),
  connectWireless: (address: string) =>
    call<string>("connect_wireless", { address }),
  disconnectWireless: (address: string) =>
    call<void>("disconnect_wireless", { address }),

  // library
  listApps: (serial: string) => call<InstalledApp[]>("list_apps", { serial }),
  getAppVersion: (serial: string, pkg: string) =>
    call<{ versionName?: string | null; versionCode?: number | null }>(
      "get_app_version",
      { serial, package: pkg },
    ),
  launchApp: (serial: string, pkg: string) =>
    call<void>("launch_app", { serial, package: pkg }),
  forceStopApp: (serial: string, pkg: string) =>
    call<void>("force_stop_app", { serial, package: pkg }),
  uninstallApp: (serial: string, pkg: string) =>
    call<void>("uninstall_app", { serial, package: pkg }),
  clearAppData: (serial: string, pkg: string) =>
    call<void>("clear_app_data", { serial, package: pkg }),

  // apk
  inspectApks: (paths: string[]) => call<ApkMetadata[]>("inspect_apks", { paths }),
  inspectApkBatch: (paths: string[]) =>
    call<ApkInspectionBatch>("inspect_apk_batch", { paths }),
  checkApkCompatibility: (serial: string, apkPath: string) =>
    call<string[]>("check_apk_compatibility", { serial, apkPath }),
  installApk: (serials: string[], apkPath: string) =>
    call<InstallJob>("install_apk", { serials, apkPath }),
  startInstallQueue: (serials: string[], apkPaths: string[]) =>
    call<InstallJob>("start_install_queue", { serials, apkPaths }),
  getInstallJob: (jobId: string) =>
    call<InstallJobSnapshot>("get_install_job", { jobId }),
  listInstallJobs: () => call<InstallJobSnapshot[]>("list_install_jobs"),
  retryInstallJob: (jobId: string) =>
    call<InstallJob>("retry_install_job", { jobId }),
  cancelJob: (jobId: string) => call<void>("cancel_job", { jobId }),
};

// ---- formatting helpers ----------------------------------------------------

export function formatBytes(bytes?: number | null): string {
  if (bytes == null) return "—";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const decimals = unit >= 2 && value < 100 ? 1 : 0;
  return `${value.toFixed(decimals)} ${units[unit]}`;
}

export function platformLabel(platform: Platform): string {
  switch (platform) {
    case "pico":
      return "Pico";
    case "quest":
      return "Meta Quest";
    case "android":
      return "Android";
    default:
      return "Unknown device";
  }
}

/** User-facing explanation for a device that is not ready. */
export function stateHint(state: DeviceState): string | null {
  switch (state) {
    case "device":
      return null;
    case "unauthorized":
      return "Put the headset on and allow USB debugging";
    case "offline":
      return "Device went offline — reconnect it";
    case "connecting":
      return "Connecting…";
    case "noPermissions":
      return "This computer cannot access the device";
    default:
      return "Not ready for commands";
  }
}
