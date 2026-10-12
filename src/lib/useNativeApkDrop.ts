import { useEffect, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { toUiError, type UiError } from "./api";

/** Native drop payloads are the only source of dropped filesystem paths. */
export function useNativeApkDrop(onPaths: (paths: string[]) => Promise<void>) {
  const [dropping, setDropping] = useState(false);
  const [error, setError] = useState<UiError | null>(null);
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void (async () => {
      try {
        const stop = await getCurrentWebview().onDragDropEvent(({ payload }) => {
          if (disposed) return;
          if (payload.type === "enter" || payload.type === "over") setDropping(true);
          else {
            setDropping(false);
            if (payload.type === "drop" && payload.paths.length) {
              void onPaths(payload.paths).catch((err) => { if (!disposed) setError(toUiError(err)); });
            }
          }
        });
        // StrictMode can clean up before the registration promise resolves.
        if (disposed) stop();
        else unlisten = stop;
      } catch (err) {
        if (!disposed) setError(toUiError(err));
      }
    })();
    return () => { disposed = true; unlisten?.(); };
  }, [onPaths]);
  return { dropping, error };
}
