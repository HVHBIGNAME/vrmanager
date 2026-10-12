import { waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { emitJob, native, resetNative } from "./native";
import { apk, deferred, devices, item, job } from "./fixtures";
import { InstallActivity } from "../../src/lib/installActivity";
import type { InstallJobSnapshot } from "../../src/lib/events";

const cleanups: (() => void)[] = [];
beforeEach(resetNative);
afterEach(() => { cleanups.splice(0).forEach((cleanup) => cleanup()); });
function mount() {
  const activity = new InstallActivity();
  const disconnect = activity.connect();
  cleanups.push(disconnect);
  return { activity, disconnect };
}

const failed = () => job({ status: "failed", items: [item({ status: "failed", stage: "failed",
  error: { kind: "offline", message: "Disconnected", retryable: true } })] });

describe("revisioned job lifecycle", () => {
  it("ignores old, duplicate and foreign updates for the selected job, including a late list", async () => {
    const history = deferred<InstallJobSnapshot[]>();
    native.handlers.list_install_jobs = () => history.promise;
    const { activity } = mount();
    await waitFor(() => expect(activity.getSnapshot().connected).toBe(true));
    const newest = job({ revision: 8, status: "completed", items: [item({ status: "succeeded", stage: "done" })] });
    emitJob(newest);
    emitJob(job({ revision: 7 }));
    emitJob(job({ revision: 8, status: "failed" }));
    emitJob(job({ jobId: "foreign", revision: 90 }));
    history.resolve([job({ revision: 1 })]);
    await waitFor(() => expect(activity.getSnapshot().pending.list).toBe(false));
    expect(activity.getSnapshot().jobs["job-a"]).toBe(newest);
    expect(activity.getSnapshot().jobs.foreign?.jobId).toBe("foreign");
  });

  it.each(["start", "retry"] as const)("awaits listener registration before %s, then fetches only the returned id", async (operation) => {
    const registration = deferred<void>();
    const listen = native.listen.getMockImplementation()!;
    native.listen.mockImplementation(async (name, callback) => {
      const off = await listen(name, callback);
      await registration.promise;
      return off;
    });
    const complete = job({ jobId: "returned-id", revision: 5, status: "completed", items: [] });
    native.handlers.list_install_jobs = () => [failed()];
    native.handlers.get_install_job = ({ jobId } = {}) => jobId === "job-a" ? failed() : job({ jobId: "returned-id" });
    const command = operation === "start" ? "start_install_queue" : "retry_install_job";
    native.handlers[command] = () => { emitJob(complete); return { jobId: "returned-id" }; };
    const { activity } = mount();
    await activity.fetch("job-a");
    const result = operation === "start" ? activity.start([devices[0].serial], [apk.path]) : activity.retry("job-a");
    await Promise.resolve();
    expect(native.invoke.mock.calls.some(([cmd]) => cmd === command)).toBe(false);
    registration.resolve();
    expect(await result).toBe("returned-id");
    expect(activity.getSnapshot().jobs["returned-id"]).toEqual(complete);
    expect(native.invoke).toHaveBeenCalledWith("get_install_job", { jobId: "returned-id" });
  });

  it("releases late registration on cleanup and does not invoke an unobserved start", async () => {
    const registration = deferred<() => void>();
    const unlisten = vi.fn();
    native.listen.mockReturnValue(registration.promise);
    native.handlers.start_install_queue = () => ({ jobId: "unobserved" });
    native.handlers.get_install_job = () => job({ jobId: "unobserved" });
    const { activity, disconnect } = mount();
    const started = activity.start([devices[0].serial], [apk.path]);
    disconnect();
    registration.resolve(unlisten);
    await started;
    expect(unlisten).toHaveBeenCalledTimes(1);
    expect(native.invoke.mock.calls.some(([cmd]) => cmd === "start_install_queue")).toBe(false);
  });

  it("ignores a fetch from the disposed subscription generation after reconnect", async () => {
    const late = deferred<InstallJobSnapshot>();
    native.handlers.list_install_jobs = () => [job()];
    native.handlers.get_install_job = () => late.promise;
    const { activity, disconnect } = mount();
    await waitFor(() => expect(activity.getSnapshot().pending.list).toBe(false));
    const fetch = activity.fetch("job-a");
    disconnect();
    cleanups.push(activity.connect());
    await waitFor(() => expect(activity.getSnapshot().connected).toBe(true));
    late.resolve(job({ revision: 99, status: "failed" }));
    await fetch;
    expect(activity.getSnapshot().jobs["job-a"]?.revision).toBe(1);
  });

  it("preserves an accepted retry id when fetching it fails", async () => {
    native.handlers.list_install_jobs = () => [failed()];
    native.handlers.retry_install_job = () => ({ jobId: "retry-accepted" });
    native.handlers.get_install_job = () => { throw "Fetch unavailable"; };
    const { activity } = mount();
    await waitFor(() => expect(activity.getSnapshot().jobs["job-a"]).toBeTruthy());
    expect(await activity.retry("job-a")).toBe("retry-accepted");
    expect(Object.keys(activity.getSnapshot().jobs)).toContain("retry-accepted");
    expect(activity.getSnapshot().issues["fetch:retry-accepted"].error.message).toBe("Fetch unavailable");
    expect(activity.getSnapshot().jobs["job-a"]?.status).toBe("failed");
  });

  it("reports a retry subscription failure without invoking retry", async () => {
    native.listen.mockRejectedValue({ kind: "events", message: "No event channel", retryable: true });
    native.handlers.list_install_jobs = () => [failed()];
    const { activity } = mount();
    await waitFor(() => expect(activity.getSnapshot().jobs["job-a"]).toBeTruthy());
    expect(await activity.retry("job-a")).toBeUndefined();
    expect(activity.getSnapshot().issues["retry:job-a"].error.message).toBe("No event channel");
    expect(native.invoke.mock.calls.some(([cmd]) => cmd === "retry_install_job")).toBe(false);
  });
});
