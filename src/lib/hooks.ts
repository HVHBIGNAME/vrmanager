import { useCallback, useEffect, useRef, useState } from "react";

import {
  api,
  type DeviceSnapshot,
  type Settings,
  type Environment,
  type UiError,
} from "../lib/api";
import { onDevicesChanged } from "../lib/events";

/**
 * Device state lives in one hook, shared by the shell and every screen.
 *
 * Event-driven on purpose: the Rust core emits `devices-changed`, so this
 * never polls ADB on a timer (performance requirement).
 */
export function useDevices() {
  const [devices, setDevices] = useState<DeviceSnapshot[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<UiError | null>(null);
  const inFlight = useRef(false);

  const refresh = useCallback(async () => {
    if (inFlight.current) return;
    inFlight.current = true;
    try {
      const list = await api.listDevices();
      setDevices(list);
      setError(null);
    } catch (err) {
      setError(err as UiError);
    } finally {
      inFlight.current = false;
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
    const unlisten = onDevicesChanged(() => void refresh());
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [refresh]);

  return { devices, loading, error, refresh };
}

/** Persisted settings, including the global device selection. */
export function useSettings() {
  const [settings, setSettings] = useState<Settings | null>(null);

  useEffect(() => {
    void api.getSettings().then(setSettings).catch(() => setSettings(null));
  }, []);

  const selectDevices = useCallback(async (serials: string[]) => {
    const next = await api.setSelectedDevices(serials);
    setSettings(next);
    return next;
  }, []);

  const setShowSystemApps = useCallback(async (show: boolean) => {
    const next = await api.setShowSystemApps(show);
    setSettings(next);
    return next;
  }, []);

  const completeOnboarding = useCallback(async () => {
    const next = await api.completeOnboarding();
    setSettings(next);
    return next;
  }, []);

  return { settings, selectDevices, setShowSystemApps, completeOnboarding };
}

/** ADB availability / version for Settings and the first-run flow. */
export function useEnvironment() {
  const [env, setEnv] = useState<Environment | null>(null);
  const [loading, setLoading] = useState(true);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      setEnv(await api.getEnvironment());
    } catch {
      setEnv(null);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  return { env, loading, refresh };
}
