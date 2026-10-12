import { vi } from "vitest";
import { apk, devices } from "./fixtures";

type Handler = (args?: Record<string, unknown>) => unknown;
const native = vi.hoisted(() => ({
  invoke: vi.fn(), listen: vi.fn(), drag: vi.fn(), picker: vi.fn(),
  listeners: new Map<string, Set<(event: { payload: unknown }) => void>>(),
  dropListeners: new Set<(event: { payload: unknown }) => void>(),
  handlers: {} as Record<string, Handler>,
}));
export { native };
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: native.listen }));
vi.mock("@tauri-apps/api/webview", () => ({ getCurrentWebview: () => ({ onDragDropEvent: native.drag }) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: native.picker }));

export function resetNative() {
  native.listeners.clear();
  native.dropListeners.clear();
  native.handlers = {
    list_devices: () => devices,
    get_settings: () => ({ selectedSerials: [devices[0].serial], showSystemApps: false, onboarded: true }),
    get_environment: () => ({ adbAvailable: true, backendConfigured: false, onboarded: true }),
    check_apk_compatibility: () => [],
    list_install_jobs: () => [],
    inspect_apks: () => [apk],
    inspect_apk_batch: () => ({ apks: [apk], errors: [] }),
  };
  native.invoke.mockReset().mockImplementation(async (cmd: string, args?: Record<string, unknown>) => {
    if (!native.handlers[cmd]) throw new Error(`Unconfigured test IPC: ${cmd}`);
    return native.handlers[cmd](args);
  });
  native.listen.mockReset().mockImplementation(async (name: string, cb: (e: { payload: unknown }) => void) => {
    const listeners = native.listeners.get(name) ?? new Set();
    native.listeners.set(name, listeners);
    listeners.add(cb);
    return vi.fn(() => { listeners.delete(cb); });
  });
  native.drag.mockReset().mockImplementation(async (cb: (e: { payload: unknown }) => void) => {
    native.dropListeners.add(cb);
    return vi.fn(() => { native.dropListeners.delete(cb); });
  });
  native.picker.mockReset().mockResolvedValue([apk.path]);
}
export function emitJob(payload: unknown) {
  for (const cb of native.listeners.get("vrmanager://install-job-changed") ?? []) cb({ payload });
}
export function emitDrop(payload: unknown) {
  for (const cb of native.dropListeners) cb({ payload });
}
