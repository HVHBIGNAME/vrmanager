import { act, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { emitDrop, native, resetNative } from "./native";
import { apk } from "./fixtures";
import App from "../../src/App";

beforeEach(resetNative);

describe("native APK intake", () => {
  it("inspects absolute native drop paths without discarding partial failures", async () => {
    const paths = [apk.path, "C:/VR apps/bad.apk"];
    native.handlers.inspect_apk_batch = () => ({ apks: [apk], errors: [{
      path: paths[1], error: { kind: "apkInvalid", message: "The manifest is damaged", retryable: false },
    }] });
    render(<App />);
    await waitFor(() => expect(native.drag).toHaveBeenCalledTimes(1));
    act(() => emitDrop({ type: "drop", paths, position: { x: 1, y: 2 } }));
    await waitFor(() => expect(native.invoke).toHaveBeenCalledWith("inspect_apk_batch", { paths }));
    expect(await screen.findByText("First app")).toBeTruthy();
    expect(screen.getByText("The manifest is damaged")).toBeTruthy();
    expect(screen.getByText(paths[1])).toBeTruthy();
    expect(native.invoke).not.toHaveBeenCalledWith("inspect_apks", expect.anything());
  });
});
