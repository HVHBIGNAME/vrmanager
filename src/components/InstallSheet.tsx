import { useEffect, useMemo, useState } from "react";
import { api, formatBytes, toUiError, type ApkMetadata, type ApkInspectionBatch, type DeviceSnapshot } from "../lib/api";
import { useInstallActivity } from "../lib/installActivity";
import { useModalFocus } from "../lib/useModalFocus";
import { ActivityIssues, InstallJobView } from "./InstallJobView";
import { IconPackage, IconX } from "./icons";

/** Metadata/target selection only; Rust owns every job after admission. */
export function InstallSheet({ apks, inspectionErrors = [], devices, defaultSerials, jobId: initialJobId, onClose }: {
  apks: ApkMetadata[];
  inspectionErrors?: ApkInspectionBatch["errors"];
  devices: DeviceSnapshot[];
  defaultSerials: string[];
  jobId?: string;
  onClose: () => void;
}) {
  const { activity, state } = useInstallActivity();
  const modalRef = useModalFocus(onClose);
  const [jobId, setJobId] = useState(initialJobId);
  const readyDevices = useMemo(() => devices.filter((device) => device.ready), [devices]);
  const [targets, setTargets] = useState<string[]>(() => {
    const selected = defaultSerials.filter((serial) => readyDevices.some((d) => d.serial === serial));
    return selected.length ? selected : readyDevices.slice(0, 1).map((d) => d.serial);
  });
  const [warnings, setWarnings] = useState<string[]>([]);
  const validTargets = targets.filter((serial) => readyDevices.some((d) => d.serial === serial));
  const starting = state.pending.start;
  const preview = jobId ? state.previews[jobId] : undefined;
  const shownApks = preview?.apks ?? apks;
  const shownErrors = preview?.errors ?? inspectionErrors;

  useEffect(() => {
    if (initialJobId) void activity.fetch(initialJobId);
  }, [activity, initialJobId]);

  useEffect(() => {
    if (jobId) return;
    let cancelled = false;
    void (async () => {
      const next: string[] = [];
      for (const serial of targets) for (const apk of apks) {
        try {
          const messages = await api.checkApkCompatibility(serial, apk.path);
          next.push(...messages.map((message) => `${apk.fileName} · ${serial}: ${message}`));
        } catch (err) {
          next.push(`${apk.fileName} · ${serial}: Compatibility check unavailable — ${toUiError(err).message}`);
        }
      }
      if (!cancelled) setWarnings(next);
    })();
    return () => { cancelled = true; };
  }, [targets, apks, jobId]);

  const start = async () => {
    if (!validTargets.length || !apks.length) return;
    const id = await activity.start(validTargets, apks.map((apk) => apk.path));
    if (id) {
      activity.rememberPreview(id, { apks, errors: inspectionErrors });
      setJobId(id);
    }
  };

  return <div className="overlay" onClick={onClose}>
    <div ref={modalRef} tabIndex={-1} className="modal modal--wide" role="dialog" aria-modal="true" aria-label={jobId ? "Installation details" : "Install APKs"}
      onClick={(event) => event.stopPropagation()}>
      <div className="modal__header">
        <div className="col" style={{ gap: 2, minWidth: 0 }}>
          <h2 className="h3">{jobId ? "Installation details" : apks.length === 1 ? "APK detected" : `${apks.length} APKs detected`}</h2>
          <div className="muted" style={{ fontSize: "var(--text-sm)" }}>
            {jobId ? "Closing this window does not cancel the job." : `${formatBytes(apks.reduce((sum, apk) => sum + apk.sizeBytes, 0))} total`}
          </div>
        </div>
        <button className="icon-btn" onClick={onClose} aria-label="Close"><IconX size={15} /></button>
      </div>
      <div className="modal__body stack-4">
        <ActivityIssues />
        {shownErrors.map(({ path, error }) => <div key={path} className="notice notice--danger" role="alert">
          <div className="mono" style={{ overflowWrap: "anywhere" }}>{path}</div><div>{error.message}</div>
          {error.details && <details className="details"><summary>Technical details</summary><div className="details__content">{error.details}</div></details>}
        </div>)}
        {shownApks.length > 0 && <div className="stack-2">
          {shownApks.map((apk) => <div key={apk.path} className="apk-row">
            <div className="apk-row__icon"><IconPackage size={16} /></div>
            <div className="col" style={{ minWidth: 0, flex: 1, gap: 1 }}>
              <div className="row" style={{ gap: "var(--space-2)" }}>
                <span className="truncate" style={{ fontWeight: 560 }}>{apkName(apk)}</span>
                {apk.vrCapable && <span className="badge badge--accent">VR</span>}
              </div>
              <span className="dim mono truncate" title={apk.path}>{apk.package}</span>
            </div>
            <div className="col" style={{ alignItems: "flex-end", gap: 1 }}>
              <span style={{ fontSize: "var(--text-sm)" }}>{apk.versionName ? `Version ${apk.versionName}` : "—"}</span>
              <span className="dim" style={{ fontSize: "var(--text-xs)" }}>{(apk.abis[0] ?? "universal").toUpperCase()} · {formatBytes(apk.sizeBytes)}</span>
            </div>
          </div>)}
        </div>}
        {jobId ? <InstallJobView jobId={jobId} devices={devices} onRetried={setJobId} /> : <>
          <div className="stack-2">
            <div className="field__label">Install to</div>
            {!readyDevices.length && <div className="muted">No ready devices. Connect a headset first.</div>}
            {readyDevices.map((device) => <div key={device.serial} className="target">
              <label className="target__head">
                <input type="checkbox" checked={targets.includes(device.serial)} disabled={starting}
                  onChange={() => setTargets((previous) => previous.includes(device.serial)
                    ? previous.filter((serial) => serial !== device.serial) : [...previous, device.serial])} />
                <span className="col" style={{ gap: 1, minWidth: 0 }}>
                  <span className="truncate">{device.displayName}</span><span className="dim">{device.modelName}</span>
                </span>
              </label>
            </div>)}
          </div>
          {warnings.length > 0 && <div className="notice notice--warning stack-2">
            <strong>Before you install</strong>{warnings.map((warning, index) => <div key={index}>{warning}</div>)}
          </div>}
        </>}
      </div>
      <div className="modal__footer">
        <button className="btn btn--ghost" onClick={onClose}>{jobId || starting ? "Run in background" : "Close"}</button>
        {!jobId && <button className="btn btn--primary" disabled={starting || !validTargets.length || !apks.length}
          onClick={() => void start()}>{starting ? "Starting…" : `Install${validTargets.length > 1 ? ` on ${validTargets.length} devices` : ""}`}</button>}
      </div>
    </div>
  </div>;
}

function apkName(apk: ApkMetadata): string {
  const label = apk.applicationLabel?.trim();
  if (label && !label.startsWith("@")) return label;
  const tail = apk.package.split(".").pop() ?? apk.package;
  return tail.charAt(0).toUpperCase() + tail.slice(1);
}
