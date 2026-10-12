import { createContext, useCallback, useContext, useMemo, useState } from "react";
import type { ReactNode } from "react";

import type { UiError } from "../lib/api";

/**
 * Toast layer.
 *
 * Errors arriving from Rust are `UiError`: a human sentence plus optional
 * technical details, which are rendered behind a disclosure so a raw ADB
 * string is never the primary message.
 */

export interface Toast {
  id: string;
  variant: "info" | "success" | "error";
  title: string;
  message?: string;
  details?: string;
}

interface ToastApi {
  show: (toast: Omit<Toast, "id">) => void;
  success: (title: string, message?: string) => void;
  error: (title: string, err?: UiError | unknown) => void;
}

const ToastContext = createContext<ToastApi | null>(null);

export function useToast(): ToastApi {
  const ctx = useContext(ToastContext);
  if (!ctx) throw new Error("useToast must be used inside <ToastProvider>");
  return ctx;
}

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([]);

  const dismiss = useCallback((id: string) => {
    setToasts((prev) => prev.filter((t) => t.id !== id));
  }, []);

  const show = useCallback(
    (toast: Omit<Toast, "id">) => {
      const id = Math.random().toString(36).slice(2);
      setToasts((prev) => [...prev.slice(-3), { ...toast, id }]);
      // Errors stay longer: the user may want to open the details.
      const ttl = toast.variant === "error" ? 9000 : 4200;
      window.setTimeout(() => dismiss(id), ttl);
    },
    [dismiss],
  );

  const value = useMemo<ToastApi>(
    () => ({
      show,
      success: (title, message) => show({ variant: "success", title, message }),
      error: (title, err) => {
        const ui = err as UiError | undefined;
        show({
          variant: "error",
          title,
          message: ui?.message,
          details: ui?.details,
        });
      },
    }),
    [show],
  );

  return (
    <ToastContext.Provider value={value}>
      {children}
      <div className="toasts">
        {toasts.map((toast) => (
          <div key={toast.id} className={`toast toast--${toast.variant}`}>
            <div className="toast__accent" />
            <div className="toast__content">
              <div className="toast__title">{toast.title}</div>
              {toast.message && (
                <div className="toast__message">{toast.message}</div>
              )}
              {toast.details && (
                <details className="details">
                  <summary>Technical details</summary>
                  <div className="details__content">{toast.details}</div>
                </details>
              )}
            </div>
            <button
              className="icon-btn"
              onClick={() => dismiss(toast.id)}
              aria-label="Dismiss"
            >
              ×
            </button>
          </div>
        ))}
      </div>
    </ToastContext.Provider>
  );
}
