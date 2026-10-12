import { beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "../../src/lib/api";
import { invoke } from "@tauri-apps/api/core";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

beforeEach(() => vi.mocked(invoke).mockReset());

describe("frozen local installation IPC", () => {
  it("uses the queue command names and exact camelCase arguments", async () => {
    const serials = ["headset-a", "headset-b"];
    const apkPaths = ["C:\\VR apps\\first.APK", "C:/VR apps/second.apk"];
    vi.mocked(invoke).mockResolvedValue({ jobId: "job-returned" });
    expect(api).toHaveProperty("startInstallQueue");
    await api.startInstallQueue(serials, apkPaths);
    await api.getInstallJob("job-returned");
    await api.listInstallJobs();
    await api.retryInstallJob("job-returned");
    await api.cancelJob("job-returned");
    expect(vi.mocked(invoke).mock.calls).toEqual([
      ["start_install_queue", { serials, apkPaths }],
      ["get_install_job", { jobId: "job-returned" }],
      ["list_install_jobs", undefined],
      ["retry_install_job", { jobId: "job-returned" }],
      ["cancel_job", { jobId: "job-returned" }],
    ]);
  });

  it("preserves exact native paths and per-file inspection failures", async () => {
    const paths = ["C:\\VR apps\\two words.APK", "C:/VR apps/bad.apk"];
    const result = {
      apks: [{ path: paths[0], package: "com.example.good" }],
      errors: [{ path: paths[1], error: { kind: "apkInvalid", message: "Invalid manifest", retryable: false } }],
    };
    vi.mocked(invoke).mockResolvedValue(result);
    expect(api).toHaveProperty("inspectApkBatch");
    expect(await api.inspectApkBatch(paths)).toEqual(result);
    expect(invoke).toHaveBeenCalledWith("inspect_apk_batch", { paths });
  });
});
