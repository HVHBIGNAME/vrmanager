import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { emitJob, native, resetNative } from "./native";
import { deferred, item, job, secondApk, devices } from "./fixtures";
import App from "../../src/App";

beforeEach(resetNative);

describe("installation controls", () => {
  it("launches only a succeeded item with a package on its exact device", async () => {
    const snapshot = job({ status: "failed", items: [
      item({ status: "succeeded", stage: "done" }),
      item({ itemId: "no-package", status: "succeeded", package: null }),
      item({ itemId: "failed", status: "failed" }),
      item({ itemId: "cancelled", status: "cancelled" }),
    ] });
    native.handlers.list_install_jobs = () => [snapshot];
    native.handlers.launch_app = () => { throw { kind: "launch", message: "Headset went offline", retryable: true }; };
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Downloads" }));
    const entry = await screen.findByRole("article", { name: "Installation job-a" });
    const launch = within(entry).getAllByRole("button", { name: /Launch First app/ });
    expect(launch).toHaveLength(1);
    fireEvent.click(launch[0]);
    await waitFor(() => expect(native.invoke).toHaveBeenCalledWith("launch_app", {
      serial: snapshot.items[0].serial, package: snapshot.items[0].package,
    }));
    expect(await screen.findByText("Could not launch app")).toBeTruthy();
    expect(screen.getByText("Headset went offline")).toBeTruthy();
    native.handlers.launch_app = () => undefined;
    fireEvent.click(launch[0]);
    await waitFor(() => expect(screen.queryByText("Could not launch app")).toBeNull());
  });

  it("retries only an eligible terminal job by id and keeps early completion of the new job", async () => {
    const retryable = item({ itemId: "failed-b", serial: devices[1].serial, apkPath: secondApk.path,
      status: "failed", stage: "failed", error: { kind: "offline", message: "Device disconnected", retryable: true } });
    const original = job({ status: "failed", items: [item({ status: "succeeded", stage: "done" }), retryable] });
    const blocked = job({ jobId: "blocked", status: "failed", items: [item({ status: "failed", stage: "failed",
      error: { kind: "invalidApk", message: "Invalid manifest", retryable: false } })] });
    const running = job({ jobId: "still-running", items: [retryable, item()] });
    const retried = job({ jobId: "retry-new", revision: 9, status: "completed", items: [
      { ...retryable, status: "succeeded", stage: "done", error: null },
    ] });
    native.handlers.list_install_jobs = () => [original, blocked, running];
    native.handlers.get_install_job = ({ jobId } = {}) => jobId === "retry-new" ? job({ jobId: "retry-new" }) : original;
    native.handlers.retry_install_job = () => { throw { kind: "busy", message: "Core is busy", retryable: true }; };
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Downloads" }));
    const entry = await screen.findByRole("article", { name: "Installation job-a" });
    expect(within(await screen.findByRole("article", { name: "Installation blocked" })).queryByRole("button", { name: "Retry eligible items" })).toBeNull();
    expect(within(await screen.findByRole("article", { name: "Installation still-running" })).queryByRole("button", { name: "Retry eligible items" })).toBeNull();
    fireEvent.click(within(entry).getByRole("button", { name: "Retry eligible items" }));
    expect(await screen.findByText("Could not retry installation")).toBeTruthy();
    expect(screen.getByText("Core is busy")).toBeTruthy();
    let subscribedBeforeRetry = false;
    native.handlers.retry_install_job = () => {
      subscribedBeforeRetry = (native.listeners.get("vrmanager://install-job-changed")?.size ?? 0) > 0;
      emitJob(retried);
      return { jobId: "retry-new" };
    };
    fireEvent.click(within(entry).getByRole("button", { name: "Retry eligible items" }));
    const dialog = await screen.findByRole("dialog", { name: "Installation details" });
    expect(within(dialog).getByText("Completed")).toBeTruthy();
    expect(subscribedBeforeRetry).toBe(true);
    expect(native.invoke).toHaveBeenCalledWith("retry_install_job", { jobId: "job-a" });
    expect(native.invoke).toHaveBeenCalledWith("get_install_job", { jobId: "retry-new" });
    expect(native.invoke.mock.calls.some(([cmd]) => cmd === "start_install_queue" || cmd === "install_apk")).toBe(false);
    expect(within(entry).getByText("Device disconnected")).toBeTruthy();
  });

  it("cancels the exact job without inventing a terminal status and surfaces a failed cancel", async () => {
    let snapshot = job();
    native.handlers.list_install_jobs = () => [snapshot];
    native.handlers.get_install_job = () => snapshot;
    const cancellation = deferred<void>();
    native.handlers.cancel_job = () => cancellation.promise;
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Downloads" }));
    const entry = await screen.findByRole("article", { name: "Installation job-a" });
    fireEvent.click(within(entry).getByRole("button", { name: "Cancel installation" }));
    await waitFor(() => expect(native.invoke).toHaveBeenCalledWith("cancel_job", { jobId: "job-a" }));
    expect((within(entry).getByRole("button", { name: "Cancel installation" }) as HTMLButtonElement).disabled).toBe(true);
    expect(within(entry).getByText("In progress")).toBeTruthy();
    await act(async () => cancellation.reject({ kind: "transport", message: "Could not reach the core", retryable: true }));
    expect(await screen.findByText("Could not cancel installation")).toBeTruthy();
    expect(screen.getByText("Could not reach the core")).toBeTruthy();
    expect(within(entry).getByText("In progress")).toBeTruthy();
    native.handlers.cancel_job = () => {
      snapshot = job({ revision: 5, status: "cancelled", items: [item({ status: "cancelled" })] });
    };
    fireEvent.click(within(entry).getByRole("button", { name: "Cancel installation" }));
    await waitFor(() => expect(within(entry).queryByRole("button", { name: "Cancel installation" })).toBeNull());
    expect(native.invoke).toHaveBeenCalledWith("get_install_job", { jobId: "job-a" });
    expect(within(entry).getAllByText("Cancelled")).toHaveLength(2);
  });
});
