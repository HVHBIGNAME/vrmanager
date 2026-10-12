import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { emitJob, native, resetNative } from "./native";
import { apk, secondApk, devices, item, job } from "./fixtures";
import App from "../../src/App";

beforeEach(resetNative);

describe("local installation activity", () => {
  it("loads real local results in Downloads and can reopen them after remount", async () => {
    const snapshot = job({ status: "failed", items: [
      item({ status: "succeeded", stage: "done" }),
      item({ itemId: "item-b", serial: devices[1].serial, apkPath: secondApk.path,
        applicationName: "Second app", status: "failed", stage: "failed",
        error: { kind: "install", message: "Headset storage is full", retryable: true, details: "INSTALL_FAILED_INSUFFICIENT_STORAGE" } }),
    ] });
    native.handlers.list_install_jobs = () => [snapshot];
    native.handlers.get_install_job = () => snapshot;
    const first = render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Downloads" }));
    expect(await screen.findByRole("heading", { name: "Local installation activity" })).toBeTruthy();
    expect(screen.getByText(/Remote downloads are not implemented/)).toBeTruthy();
    const entry = await screen.findByRole("article", { name: "Installation job-a" });
    expect(within(entry).getByText("1 / 2 installed")).toBeTruthy();
    expect(within(entry).getByRole("listitem", { name: "First app on Headset A" })).toBeTruthy();
    expect(within(entry).getByRole("listitem", { name: "Second app on Headset B" })).toBeTruthy();
    expect(within(entry).getByText("Headset storage is full")).toBeTruthy();
    first.unmount();
    const before = native.invoke.mock.calls.filter(([cmd]) => cmd === "list_install_jobs").length;
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Downloads" }));
    await waitFor(() => expect(native.invoke.mock.calls.filter(([cmd]) => cmd === "list_install_jobs").length).toBeGreaterThan(before));
    fireEvent.click(await screen.findByRole("button", { name: "Open details for job-a" }));
    const dialog = await screen.findByRole("dialog", { name: "Installation details" });
    expect(within(dialog).getByText("Headset storage is full")).toBeTruthy();
    await waitFor(() => expect(native.invoke).toHaveBeenCalledWith("get_install_job", { jobId: "job-a" }));
    fireEvent.click(within(dialog).getByRole("button", { name: "Close" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(native.invoke.mock.calls.some(([cmd]) => cmd === "cancel_job")).toBe(false);
  });

  it("keeps completion emitted before start returns and submits the whole queue once", async () => {
    const completed = job({ revision: 8, status: "completed", items: [
      item({ status: "succeeded", stage: "done", percent: 100 }),
      item({ itemId: "item-b", serial: devices[1].serial, apkPath: secondApk.path,
        applicationName: "Second app", package: secondApk.package, status: "succeeded", stage: "done", percent: 100 }),
    ] });
    native.handlers.inspect_apk_batch = () => ({ apks: [apk, secondApk], errors: [] });
    let subscribedBeforeStart = false;
    native.handlers.start_install_queue = () => {
      subscribedBeforeStart = (native.listeners.get("vrmanager://install-job-changed")?.size ?? 0) > 0;
      emitJob(completed);
      return { jobId: completed.jobId };
    };
    native.handlers.get_install_job = () => job({ revision: 1 });
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Install APK" }));
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(within(dialog).getByRole("checkbox", { name: /Headset B/ }));
    fireEvent.click(within(dialog).getByRole("button", { name: /Install on 2 devices/ }));
    await waitFor(() => expect(native.invoke).toHaveBeenCalledWith("start_install_queue", {
      serials: devices.map((d) => d.serial), apkPaths: [apk.path, secondApk.path],
    }));
    expect(subscribedBeforeStart).toBe(true);
    expect(await within(dialog).findByText("Completed")).toBeTruthy();
    expect(native.invoke).toHaveBeenCalledWith("get_install_job", { jobId: completed.jobId });
    expect(native.invoke.mock.calls.filter(([cmd]) => cmd === "start_install_queue")).toHaveLength(1);
    expect(native.invoke.mock.calls.some(([cmd]) => cmd === "install_apk")).toBe(false);
    act(() => emitJob(job({ jobId: "foreign-job", revision: 99, status: "failed", items: [] })));
    expect(within(dialog).getByText("Completed")).toBeTruthy();
  });
});
