import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { native, resetNative } from "./native";
import { deferred, item, job } from "./fixtures";
import App from "../../src/App";

beforeEach(resetNative);

describe("activity failure recovery", () => {
  it("reports a rejected start and permits a deliberate second attempt", async () => {
    native.handlers.start_install_queue = () => { throw { kind: "admission", message: "Queue is full", retryable: true }; };
    native.handlers.get_install_job = () => job();
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Install APK" }));
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "Install" }));
    expect(await within(dialog).findByText("Could not start installation")).toBeTruthy();
    expect(within(dialog).getByText("Queue is full")).toBeTruthy();
    expect(native.invoke.mock.calls.some(([cmd]) => cmd === "get_install_job")).toBe(false);
    native.handlers.start_install_queue = () => ({ jobId: "job-a" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Install" }));
    expect(await within(dialog).findByText("In progress")).toBeTruthy();
    expect(within(dialog).queryByText("Could not start installation")).toBeNull();
  });

  it("shows subscription failure, reads history anyway, and never starts without a listener", async () => {
    const listen = native.listen.getMockImplementation()!;
    native.listen.mockImplementation((name, callback) => name === "vrmanager://install-job-changed"
      ? Promise.reject({ kind: "events", message: "Event channel unavailable", retryable: true }) : listen(name, callback));
    native.handlers.start_install_queue = () => ({ jobId: "job-a" });
    native.handlers.get_install_job = () => job();
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Install APK" }));
    const dialog = await screen.findByRole("dialog");
    expect(await within(dialog).findByText("Live installation updates unavailable")).toBeTruthy();
    await waitFor(() => expect(native.invoke).toHaveBeenCalledWith("list_install_jobs", undefined));
    fireEvent.click(within(dialog).getByRole("button", { name: "Install" }));
    expect(await within(dialog).findByText("Could not start installation")).toBeTruthy();
    expect(native.invoke.mock.calls.some(([cmd]) => cmd === "start_install_queue")).toBe(false);
    native.listen.mockImplementation(listen);
    fireEvent.click(within(dialog).getByRole("button", { name: "Reload activity" }));
    await waitFor(() => expect(within(dialog).queryByText("Live installation updates unavailable")).toBeNull());
    fireEvent.click(within(dialog).getByRole("button", { name: "Install" }));
    expect(await within(dialog).findByText("In progress")).toBeTruthy();
  });

  it("distinguishes list errors, loading and an honestly empty local history", async () => {
    native.handlers.list_install_jobs = () => { throw "History is unavailable"; };
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Downloads" }));
    expect(await screen.findByText("Could not load installation activity")).toBeTruthy();
    expect(screen.getByText("History is unavailable")).toBeTruthy();
    expect(screen.queryByText("No local installations yet")).toBeNull();
    const history = deferred<ReturnType<typeof job>[]>();
    native.handlers.list_install_jobs = () => history.promise;
    fireEvent.click(screen.getByRole("button", { name: "Refresh activity" }));
    expect(await screen.findByText("Loading installation activity…")).toBeTruthy();
    await act(async () => history.resolve([]));
    expect(await screen.findByText("No local installations yet")).toBeTruthy();
  });

  it("retains an accepted id when its fetch fails and clears that failure on list recovery", async () => {
    native.handlers.start_install_queue = () => ({ jobId: "accepted-id" });
    native.handlers.get_install_job = () => { throw { kind: "transport", message: "Status read failed", retryable: true }; };
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Install APK" }));
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "Install" }));
    expect(await within(dialog).findByText("Could not read installation status")).toBeTruthy();
    expect(within(dialog).getByText("accepted-id")).toBeTruthy();
    expect(within(dialog).queryByRole("button", { name: "Install" })).toBeNull();
    fireEvent.click(within(dialog).getByRole("button", { name: "Close" }));
    fireEvent.click(screen.getByRole("button", { name: "Downloads" }));
    const entry = await screen.findByRole("article", { name: "Installation accepted-id" });
    expect(within(entry).getByRole("button", { name: "Reload status" })).toBeTruthy();
    native.handlers.list_install_jobs = () => [job({ jobId: "accepted-id", status: "completed", items: [item({ status: "succeeded", stage: "done" })] })];
    await waitFor(() => expect((screen.getByRole("button", { name: "Refresh activity" }) as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(screen.getByRole("button", { name: "Refresh activity" }));
    expect(await within(entry).findByText("Completed")).toBeTruthy();
    await waitFor(() => expect(screen.queryByText("Could not read installation status")).toBeNull());
    expect(native.invoke.mock.calls.filter(([cmd]) => cmd === "start_install_queue")).toHaveLength(1);
  });
});
