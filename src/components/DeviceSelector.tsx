import { useEffect, useMemo, useRef, useState } from "react";

import {
  formatBytes,
  platformLabel,
  stateHint,
  type DeviceSnapshot,
} from "../lib/api";
import { IconCheck, IconChevron, IconHeadset, IconUsb, IconWifi } from "./icons";

/**
 * Global device selector.
 *
 * Supports "All devices", a single device, or a multi-device selection — the
 * catalog, install targets and compatibility all follow this choice.
 * Identical models stay distinguishable by the user's own name.
 */
export function DeviceSelector({
  devices,
  selected,
  onChange,
}: {
  devices: DeviceSnapshot[];
  selected: string[];
  onChange: (serials: string[]) => void;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  const label = useMemo(() => {
    if (devices.length === 0) return "No devices";
    if (selected.length === 0) return "All devices";
    if (selected.length === 1) {
      const device = devices.find((d) => d.serial === selected[0]);
      return device ? device.displayName : "All devices";
    }
    return `${selected.length} devices selected`;
  }, [devices, selected]);

  const toggle = (serial: string) => {
    // Multi-select: ticking devices builds the active group.
    onChange(
      selected.includes(serial)
        ? selected.filter((s) => s !== serial)
        : [...selected, serial],
    );
  };

  return (
    <div className="selector" ref={ref}>
      <button
        className="selector__trigger"
        onClick={() => setOpen((v) => !v)}
        disabled={devices.length === 0}
      >
        <IconHeadset size={15} />
        <span className="truncate">{label}</span>
        <IconChevron
          size={14}
          style={{
            transform: open ? "rotate(180deg)" : "none",
            transition: "transform var(--dur-fast) var(--ease-out)",
            opacity: 0.6,
          }}
        />
      </button>

      {open && (
        <div className="selector__menu">
          <button
            className={`selector__item ${selected.length === 0 ? "is-active" : ""}`}
            onClick={() => {
              onChange([]);
              setOpen(false);
            }}
          >
            <span className="selector__check">
              {selected.length === 0 && <IconCheck size={13} />}
            </span>
            <span className="col" style={{ minWidth: 0, flex: 1 }}>
              <span className="truncate">All devices</span>
              <span className="dim" style={{ fontSize: "var(--text-xs)" }}>
                {devices.length} connected
              </span>
            </span>
          </button>

          <div className="selector__divider" />

          {devices.map((device) => {
            const hint = stateHint(device.state);
            return (
              <button
                key={device.serial}
                className={`selector__item ${
                  selected.includes(device.serial) ? "is-active" : ""
                }`}
                onClick={() => toggle(device.serial)}
              >
                <span className="selector__check">
                  {selected.includes(device.serial) && <IconCheck size={13} />}
                </span>
                <span className="col" style={{ minWidth: 0, flex: 1 }}>
                  <span className="row" style={{ gap: "var(--space-2)" }}>
                    <span
                      className={`dot ${
                        device.ready
                          ? "dot--online"
                          : device.state === "unauthorized"
                            ? "dot--warning"
                            : "dot--offline"
                      }`}
                    />
                    <span className="truncate">{device.displayName}</span>
                  </span>
                  <span
                    className="dim truncate"
                    style={{ fontSize: "var(--text-xs)", paddingLeft: 15 }}
                  >
                    {hint ?? `${device.modelName} · ${platformLabel(device.platform)}`}
                  </span>
                </span>
                <span className="dim" style={{ display: "flex", flex: "none" }}>
                  {device.connection === "usb" ? (
                    <IconUsb size={13} />
                  ) : (
                    <IconWifi size={13} />
                  )}
                </span>
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}

/** Device card used on the Devices screen. */
export function DeviceCard({
  device,
  onManage,
}: {
  device: DeviceSnapshot;
  onManage: () => void;
}) {
  const hint = stateHint(device.state);

  return (
    <div className="card card--interactive" onClick={onManage}>
      <div className="card__body stack-4">
        <div className="row row--between" style={{ alignItems: "flex-start" }}>
          <div className="col" style={{ minWidth: 0, gap: 2 }}>
            <div className="section-title truncate">{device.displayName}</div>
            <div className="dim truncate" style={{ fontSize: "var(--text-sm)" }}>
              {device.modelName}
            </div>
          </div>
          <div className="device-glyph">
            <IconHeadset size={18} />
          </div>
        </div>

        <div className="row" style={{ gap: "var(--space-2)" }}>
          <span className={`badge ${device.ready ? "badge--success" : "badge--warning"}`}>
            {device.connection === "usb" ? <IconUsb size={11} /> : <IconWifi size={11} />}
            {device.ready ? `Connected via ${device.connection === "usb" ? "USB" : "Wi-Fi"}` : "Not ready"}
          </span>
          <span className="badge">{platformLabel(device.platform)}</span>
        </div>

        {hint ? (
          <div className="muted" style={{ fontSize: "var(--text-sm)" }}>
            {hint}
          </div>
        ) : (
          <div className="row" style={{ gap: "var(--space-6)" }}>
            <Stat
              label="Battery"
              value={
                device.batteryPercent != null ? `${device.batteryPercent}%` : "—"
              }
            />
            <Stat label="Free storage" value={formatBytes(device.storageFreeBytes)} />
          </div>
        )}

        <button className="btn btn--block" onClick={onManage}>
          Manage
        </button>
      </div>
    </div>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="col" style={{ gap: 1 }}>
      <span className="dim" style={{ fontSize: "var(--text-xs)" }}>
        {label}
      </span>
      <span style={{ fontSize: "var(--text-md)", fontWeight: 560 }}>{value}</span>
    </div>
  );
}
