import { useCallback, useEffect, useState } from "react";

import {
  api,
  formatBytes,
  platformLabel,
  stateHint,
  type Device,
  type DeviceSnapshot,
  type InstalledApp,
} from "../lib/api";
import { DeviceCard } from "../components/DeviceSelector";
import { useToast } from "../components/Toast";
import {
  IconBattery,
  IconHeadset,
  IconPlay,
  IconRefresh,
  IconSearch,
  IconStop,
  IconStorage,
  IconTrash,
  IconUsb,
  IconWifi,
} from "../components/icons";

/** Devices screen: the fleet, plus the per-device dashboard. */
export function DevicesScreen({
  devices,
  loading,
  onRefresh,
}: {
  devices: DeviceSnapshot[];
  loading: boolean;
  onRefresh: () => void;
}) {
  const [openSerial, setOpenSerial] = useState<string | null>(null);

  if (loading && devices.length === 0) {
    return (
      <div className="grid">
        {[0, 1, 2].map((i) => (
          <div key={i} className="skeleton" style={{ height: 184 }} />
        ))}
      </div>
    );
  }

  if (devices.length === 0) {
    return (
      <div className="empty">
        <div className="empty__icon">
          <IconHeadset size={22} />
        </div>
        <div className="empty__title">No devices connected</div>
        <div className="empty__text">
          Connect your Pico or Quest with a USB cable and allow USB debugging on
          the headset. VRManager will pick it up automatically.
        </div>
        <button className="btn" onClick={onRefresh}>
          <IconRefresh size={14} />
          Scan again
        </button>
      </div>
    );
  }

  return (
    <>
      <div className="grid">
        {devices.map((device) => (
          <DeviceCard
            key={device.serial}
            device={device}
            onManage={() => setOpenSerial(device.serial)}
          />
        ))}
      </div>
      {openSerial && (
        <DeviceDashboard
          serial={openSerial}
          onClose={() => setOpenSerial(null)}
          onChanged={onRefresh}
        />
      )}
    </>
  );
}

/** Per-device dashboard: facts, rename, wireless, app count. */
function DeviceDashboard({
  serial,
  onClose,
  onChanged,
}: {
  serial: string;
  onClose: () => void;
  onChanged: () => void;
}) {
  const toast = useToast();
  const [device, setDevice] = useState<Device | null>(null);
  const [apps, setApps] = useState<InstalledApp[] | null>(null);
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    void (async () => {
      try {
        const d = await api.getDevice(serial);
        setDevice(d);
        setName(d.customName ?? "");
        setApps(await api.listApps(serial));
      } catch (err) {
        toast.error("Could not open this device", err);
        onClose();
      }
    })();
  }, [serial, toast, onClose]);

  const rename = async () => {
    if (!device) return;
    setBusy(true);
    try {
      await api.renameDevice(serial, name);
      toast.success("Name saved", `This headset is now “${name || device.modelName}”.`);
      onChanged();
    } catch (err) {
      toast.error("Could not rename the device", err);
    } finally {
      setBusy(false);
    }
  };

  const goWireless = async () => {
    setBusy(true);
    try {
      const addr = await api.enableWireless(serial);
      toast.success("Wi-Fi enabled", `You can unplug the cable — connected at ${addr}.`);
      onChanged();
    } catch (err) {
      toast.error("Could not switch to Wi-Fi", err);
    } finally {
      setBusy(false);
    }
  };

  const hint = device ? stateHint(device.state) : null;
  const can = (c: string) => device?.capabilities.includes(c as never) ?? false;

  return (
    <div className="overlay" onClick={onClose}>
      <div className="modal modal--wide" onClick={(e) => e.stopPropagation()}>
        {!device ? (
          <div className="modal__body" style={{ paddingTop: "var(--space-5)" }}>
            <div className="skeleton" style={{ height: 120 }} />
          </div>
        ) : (
          <>
            <div className="modal__header">
              <div className="col" style={{ gap: 2, minWidth: 0 }}>
                <div className="h3 truncate">
                  {device.customName?.trim() || device.modelName}
                </div>
                <div className="row dim" style={{ gap: "var(--space-2)", fontSize: "var(--text-sm)" }}>
                  <span>{device.modelName}</span>
                  <span>·</span>
                  <span className="row" style={{ gap: 4 }}>
                    {device.connection === "usb" ? <IconUsb size={12} /> : <IconWifi size={12} />}
                    {device.connection === "usb" ? "USB" : "Wi-Fi"}
                  </span>
                </div>
              </div>
              <span className={`badge ${device.state === "device" ? "badge--success" : "badge--warning"}`}>
                {device.state === "device" ? "Connected" : (hint ?? "Not ready")}
              </span>
            </div>

            <div className="modal__body stack-4">
              <div className="row" style={{ gap: "var(--space-3)" }}>
                <Tile
                  icon={<IconBattery size={15} />}
                  label="Battery"
                  value={device.info.batteryPercent != null ? `${device.info.batteryPercent}%` : "—"}
                />
                <Tile
                  icon={<IconStorage size={15} />}
                  label="Free storage"
                  value={formatBytes(device.info.storageFreeBytes)}
                />
                <Tile
                  icon={<IconHeadset size={15} />}
                  label="Applications"
                  value={apps ? String(apps.length) : "…"}
                />
              </div>

              <div className="field">
                <label className="field__label">Device name</label>
                <div className="row">
                  <input
                    className="input"
                    value={name}
                    placeholder={device.modelName}
                    onChange={(e) => setName(e.target.value)}
                    onKeyDown={(e) => e.key === "Enter" && void rename()}
                  />
                  <button className="btn" disabled={busy} onClick={() => void rename()}>
                    Save
                  </button>
                </div>
                <span className="dim" style={{ fontSize: "var(--text-xs)" }}>
                  Useful when you own several identical headsets.
                </span>
              </div>

              <div className="info-grid">
                <Row label="Platform" value={platformLabel(device.platform)} />
                <Row label="System version" value={device.info.androidVersion || "—"} />
                <Row label="Architecture" value={device.info.abi || "—"} />
                {device.info.ipAddress && (
                  <Row label="Address" value={device.info.ipAddress} mono />
                )}
              </div>
            </div>

            <div className="modal__footer">
              {device.connection === "usb" && can("wirelessAdb") && (
                <button
                  className="btn"
                  style={{ marginRight: "auto" }}
                  disabled={busy}
                  onClick={() => void goWireless()}
                >
                  <IconWifi size={14} />
                  Connect via Wi-Fi
                </button>
              )}
              <button className="btn btn--primary" onClick={onClose}>
                Done
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}

function Tile({
  icon,
  label,
  value,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
}) {
  return (
    <div className="tile">
      <span className="tile__icon">{icon}</span>
      <span className="dim" style={{ fontSize: "var(--text-xs)" }}>
        {label}
      </span>
      <span style={{ fontSize: "var(--text-lg)", fontWeight: 600 }}>{value}</span>
    </div>
  );
}

function Row({ label, value, mono }: { label: string; value: string; mono?: boolean }) {
  return (
    <>
      <span className="dim" style={{ fontSize: "var(--text-sm)" }}>
        {label}
      </span>
      <span className={mono ? "mono" : ""} style={{ fontSize: "var(--text-sm)" }}>
        {value}
      </span>
    </>
  );
}

/** Library: apps installed on the selected device. */
export function LibraryScreen({
  devices,
  selected,
}: {
  devices: DeviceSnapshot[];
  selected: string[];
}) {
  const toast = useToast();
  const target = selected[0] ?? devices.find((d) => d.ready)?.serial ?? null;
  const device = devices.find((d) => d.serial === target) ?? null;

  const [apps, setApps] = useState<InstalledApp[] | null>(null);
  const [query, setQuery] = useState("");
  const [busyPkg, setBusyPkg] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!target) {
      setApps(null);
      return;
    }
    try {
      setApps(await api.listApps(target));
    } catch (err) {
      toast.error("Could not read the app list", err);
      setApps([]);
    }
  }, [target, toast]);

  useEffect(() => {
    void load();
  }, [load]);

  if (!device) {
    return (
      <div className="empty">
        <div className="empty__icon">
          <IconHeadset size={22} />
        </div>
        <div className="empty__title">No device selected</div>
        <div className="empty__text">
          Connect a headset to see everything installed on it.
        </div>
      </div>
    );
  }

  const filtered = (apps ?? []).filter((a) =>
    a.package.toLowerCase().includes(query.toLowerCase()),
  );

  const act = async (pkg: string, fn: () => Promise<void>, ok: string) => {
    setBusyPkg(pkg);
    try {
      await fn();
      toast.success(ok);
      await load();
    } catch (err) {
      toast.error("That did not work", err);
    } finally {
      setBusyPkg(null);
    }
  };

  return (
    <div className="stack-4">
      <div className="row row--between">
        <div className="col" style={{ gap: 2 }}>
          <div className="section-title">Installed on {device.displayName}</div>
          <span className="dim" style={{ fontSize: "var(--text-sm)" }}>
            {apps ? `${apps.length} applications` : "Reading…"}
          </span>
        </div>
        <div className="search" style={{ width: 240 }}>
          <span className="search__icon">
            <IconSearch size={14} />
          </span>
          <input
            className="input"
            placeholder="Search apps"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        </div>
      </div>

      {apps === null ? (
        <div className="stack-2">
          {[0, 1, 2, 3].map((i) => (
            <div key={i} className="skeleton" style={{ height: 56 }} />
          ))}
        </div>
      ) : filtered.length === 0 ? (
        <div className="empty">
          <div className="empty__title">
            {query ? "Nothing matches that search" : "No user apps installed"}
          </div>
          <div className="empty__text">
            Drop an APK anywhere in the window to install it on this headset.
          </div>
        </div>
      ) : (
        <div className="stack-2">
          {filtered.map((app) => (
            <div key={app.package} className="app-row">
              <div className="col" style={{ minWidth: 0, flex: 1, gap: 1 }}>
                <span className="truncate" style={{ fontWeight: 540 }}>
                  {prettyName(app.package)}
                </span>
                <span className="dim mono truncate">{app.package}</span>
              </div>
              <div className="row" style={{ gap: "var(--space-1)" }}>
                <button
                  className="btn btn--sm"
                  disabled={busyPkg === app.package}
                  onClick={() =>
                    void act(app.package, () => api.launchApp(device.serial, app.package), "Launched")
                  }
                >
                  <IconPlay size={12} />
                  Launch
                </button>
                <button
                  className="icon-btn"
                  title="Force stop"
                  disabled={busyPkg === app.package}
                  onClick={() =>
                    void act(app.package, () => api.forceStopApp(device.serial, app.package), "Stopped")
                  }
                >
                  <IconStop size={14} />
                </button>
                <button
                  className="icon-btn"
                  title="Uninstall"
                  disabled={busyPkg === app.package}
                  onClick={() =>
                    void act(
                      app.package,
                      () => api.uninstallApp(device.serial, app.package),
                      "Uninstalled",
                    )
                  }
                >
                  <IconTrash size={14} />
                </button>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function prettyName(pkg: string): string {
  const tail = pkg.split(".").pop() ?? pkg;
  return tail.charAt(0).toUpperCase() + tail.slice(1);
}
