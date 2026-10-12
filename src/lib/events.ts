/** Event channel names and payloads pushed from Rust. */
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type { UiError } from "./api";

export const channel = {
  devicesChanged: "vrmanager://devices-changed",
  installJobChanged: "vrmanager://install-job-changed",
} as const;

export type JobStatus =
  | "queued"
  | "running"
  | "completed"
  | "failed"
  | "cancelled";

export type ItemStatus =
  | "queued"
  | "running"
  | "succeeded"
  | "failed"
  | "cancelled";

export interface InstallItemSnapshot {
  itemId: string;
  serial: string;
  apkPath: string;
  package: string | null;
  applicationName: string | null;
  status: ItemStatus;
  stage: InstallStage;
  percent: number | null;
  error: UiError | null;
}

/**
 * The whole state of one installation job.
 *
 * `revision` is monotonic, so a late-arriving event can never overwrite newer
 * state — the UI simply ignores a snapshot whose revision it has already seen.
 */
export interface InstallJobSnapshot {
  jobId: string;
  revision: number;
  status: JobStatus;
  createdAtMs: number;
  updatedAtMs: number;
  items: InstallItemSnapshot[];
}

export type InstallStage =
  | "preparing"
  | "transferring"
  | "installing"
  | "verifying"
  | "done"
  | "failed";

export const stageLabel: Record<InstallStage, string> = {
  preparing: "Preparing",
  transferring: "Copying to device",
  installing: "Installing",
  verifying: "Finishing up",
  done: "Installed",
  failed: "Failed",
};

export function onDevicesChanged(cb: () => void): Promise<UnlistenFn> {
  return listen(channel.devicesChanged, () => cb());
}

/** Whole-snapshot updates for an installation job. */
export function onInstallJobChanged(
  cb: (snapshot: InstallJobSnapshot) => void,
): Promise<UnlistenFn> {
  return listen<InstallJobSnapshot>(channel.installJobChanged, (e) =>
    cb(e.payload),
  );
}
