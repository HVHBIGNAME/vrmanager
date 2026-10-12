import type { ApkMetadata, DeviceSnapshot } from "../../src/lib/api";
import type { InstallItemSnapshot, InstallJobSnapshot } from "../../src/lib/events";

export const apk: ApkMetadata = {
  path: "C:\\VR apps\\first.APK", fileName: "first.APK", package: "com.example.first",
  applicationLabel: "First app", versionName: "1.0", sizeBytes: 1024,
  abis: ["arm64-v8a"], permissions: [], hasIcon: false, vrCapable: true,
};
export const secondApk: ApkMetadata = {
  ...apk, path: "C:/VR apps/second.apk", fileName: "second.apk",
  package: "com.example.second", applicationLabel: "Second app",
};
export const devices: DeviceSnapshot[] = ["a", "b"].map((id) => ({
  serial: `headset-${id}`, displayName: `Headset ${id.toUpperCase()}`, modelName: "Quest",
  platform: "quest", connection: "usb", state: "device", ready: true,
}));
export function item(overrides: Partial<InstallItemSnapshot> = {}): InstallItemSnapshot {
  return {
    itemId: "item-a", serial: devices[0].serial, apkPath: apk.path, package: apk.package,
    applicationName: apk.applicationLabel!, status: "running", stage: "installing",
    percent: null, error: null, ...overrides,
  };
}
export function job(overrides: Partial<InstallJobSnapshot> = {}): InstallJobSnapshot {
  return {
    jobId: "job-a", revision: 1, status: "running", createdAtMs: 1000, updatedAtMs: 1001,
    items: [item()], ...overrides,
  };
}
export function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
