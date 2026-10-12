import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { native, resetNative, emitJob } from "./native";
import { job } from "./fixtures";
import App from "../../src/App";

beforeEach(resetNative);

describe("installation keyboard access", () => {
  it("traps Tab, closes with Escape, and restores focus without cancelling the active job", async () => {
    native.handlers.start_install_queue = () => ({ jobId: "job-a" });
    native.handlers.get_install_job = () => job();
    render(<App />);
    const trigger = screen.getByRole("button", { name: "Install APK" });
    trigger.focus();
    fireEvent.click(trigger);
    let dialog = await screen.findByRole("dialog", { name: "Install APKs" });
    const close = within(dialog).getAllByRole("button", { name: "Close" })[0];
    const install = within(dialog).getByRole("button", { name: "Install" });
    expect(document.activeElement).toBe(close);

    fireEvent.keyDown(close, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(install);
    fireEvent.keyDown(install, { key: "Tab" });
    expect(document.activeElement).toBe(close);
    fireEvent.keyDown(close, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(document.activeElement).toBe(trigger);
    fireEvent.click(trigger);
    dialog = await screen.findByRole("dialog", { name: "Install APKs" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Install" }));
    expect(await within(dialog).findByText("In progress")).toBeTruthy();
    fireEvent.keyDown(document.activeElement!, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(document.activeElement).toBe(trigger);
    act(() => emitJob(job({ revision: 2, status: "cancelled", items: [] })));
    expect(native.invoke.mock.calls.some(([cmd]) => cmd === "cancel_job")).toBe(false);
    fireEvent.click(screen.getByRole("button", { name: "Downloads" }));
    expect(await screen.findByRole("article", { name: "Installation job-a" })).toBeTruthy();
  });
});
