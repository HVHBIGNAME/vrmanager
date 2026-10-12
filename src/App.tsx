import { useCallback, useEffect, useState } from "react";

import { api, toUiError, type ApkMetadata, type ApkInspectionBatch, type UiError } from "./lib/api";
import { useDevices, useEnvironment, useSettings } from "./lib/hooks";
import { useNativeApkDrop } from "./lib/useNativeApkDrop";
import { InstallActivityProvider } from "./lib/installActivity";
import { DeviceSelector } from "./components/DeviceSelector";
import { InstallSheet } from "./components/InstallSheet";
import { ToastProvider, useToast } from "./components/Toast";
import { DevicesScreen, LibraryScreen } from "./screens/Screens";
import { ActivityScreen } from "./screens/ActivityScreen";
import {
  IconCompass,
  IconDownload,
  IconHeadset,
  IconLibrary,
  IconPackage,
  IconPlus,
  IconRefresh,
  IconSettings,
  IconStar,
} from "./components/icons";

type Route = "discover" | "library" | "devices" | "downloads" | "favorites" | "settings";

const NAV: { id: Route; label: string; icon: typeof IconCompass }[] = [
  { id: "discover", label: "Discover", icon: IconCompass },
  { id: "library", label: "Library", icon: IconLibrary },
  { id: "devices", label: "Devices", icon: IconHeadset },
  { id: "downloads", label: "Downloads", icon: IconDownload },
  { id: "favorites", label: "Favorites", icon: IconStar },
];

export default function App() {
  return (
    <ToastProvider>
      <InstallActivityProvider><Shell /></InstallActivityProvider>
    </ToastProvider>
  );
}

function Shell() {
  const toast = useToast();
  const [route, setRoute] = useState<Route>("devices");
  const { devices, loading, refresh } = useDevices();
  const { settings, selectDevices } = useSettings();
  const { env, refresh: refreshEnv } = useEnvironment();

  const [pending, setPending] = useState<ApkMetadata[] | null>(null);
  const [activityJobId, setActivityJobId] = useState<string | null>(null);
  const [inspectionErrors, setInspectionErrors] = useState<ApkInspectionBatch["errors"]>([]);
  const [intakeError, setIntakeError] = useState<UiError | null>(null);
  const [inspecting, setInspecting] = useState(false);

  const selected = settings?.selectedSerials ?? [];

  // ---- drag & drop APK -----------------------------------------------------
  const handleFiles = useCallback(
    async (paths: string[]) => {
      setInspecting(true);
      setIntakeError(null);
      try {
        const result = await api.inspectApkBatch(paths);
        setInspectionErrors(result.errors);
        setActivityJobId(null);
        setPending(result.apks);
      } catch (err) {
        setIntakeError(toUiError(err));
      } finally {
        setInspecting(false);
      }
    },
    [],
  );

  const { dropping, error: dropError } = useNativeApkDrop(handleFiles);

  const pickApk = async () => {
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const result = await open({
        multiple: true,
        filters: [{ name: "Android app package", extensions: ["apk"] }],
      });
      const paths = Array.isArray(result) ? result : result ? [result] : [];
      if (paths.length > 0) await handleFiles(paths);
    } catch (err) {
      setIntakeError(toUiError(err));
    }
  };

  const ready = devices.filter((d) => d.ready);

  return (
    <div className="app">
      {/* ---- sidebar ---- */}
      <aside className="sidebar">
        <div className="brand">
          <div className="brand__mark">
            <IconHeadset size={16} />
          </div>
          <div className="col" style={{ gap: 0 }}>
            <span className="brand__name">VRManager</span>
            <span className="brand__tag">Device &amp; app manager</span>
          </div>
        </div>

        <nav className="nav">
          {NAV.map(({ id, label, icon: Icon }) => (
            <button
              key={id}
              className={`nav__item ${route === id ? "is-active" : ""}`}
              onClick={() => setRoute(id)}
            >
              <Icon size={15} />
              <span>{label}</span>
              {id === "devices" && ready.length > 0 && (
                <span className="nav__count">{ready.length}</span>
              )}
            </button>
          ))}
        </nav>

        <div className="spacer" />

        <button
          className={`nav__item ${route === "settings" ? "is-active" : ""}`}
          onClick={() => setRoute("settings")}
        >
          <IconSettings size={15} />
          <span>Settings</span>
        </button>
      </aside>

      {/* ---- main ---- */}
      <div className="main">
        <header className="topbar">
          <div className="topbar__title">
            {NAV.find((n) => n.id === route)?.label ?? "Settings"}
          </div>
          <div className="spacer" />
          <button className="btn btn--sm" disabled={inspecting} onClick={() => void pickApk()}>
            <IconPlus size={13} />
            Install APK
          </button>
          <button
            className="icon-btn"
            title="Rescan devices"
            onClick={() => {
              refresh();
              void refreshEnv();
            }}
          >
            <IconRefresh size={15} />
          </button>
          <DeviceSelector
            devices={devices}
            selected={selected}
            onChange={(serials) => void selectDevices(serials)}
          />
        </header>

        <main className="content">
          {inspecting && <p role="status">Reading APK files…</p>}
          {dropError && <div role="alert" className="notice notice--danger">Native file drop unavailable: {dropError.message}. Use Install APK to choose files.</div>}
          {intakeError && <div role="alert" className="notice notice--danger">Could not read APK files: {intakeError.message}</div>}
          {env && !env.adbAvailable && (
            <div className="notice notice--warning" style={{ marginBottom: "var(--space-5)" }}>
              <strong>Device tools not found.</strong> VRManager needs the Android
              platform tools to talk to your headset. Set the path in Settings.
            </div>
          )}

          {route === "devices" && (
            <DevicesScreen devices={devices} loading={loading} onRefresh={refresh} />
          )}

          {route === "library" && (
            <LibraryScreen devices={devices} selected={selected} />
          )}

          {route === "discover" && <Placeholder title="Discover" />}
          {route === "downloads" && <ActivityScreen devices={devices} onOpen={setActivityJobId} />}
          {route === "favorites" && <Placeholder title="Favorites" />}
          {route === "settings" && <SettingsScreen />}
        </main>
      </div>

      {/* ---- drop target ---- */}
      {dropping && (
        <div className="dropzone">
          <div className="dropzone__card">
            <IconPackage size={28} />
            <div className="h3">Drop to install</div>
            <div className="muted">Release the .apk file to read it</div>
          </div>
        </div>
      )}

      {activityJobId && !pending && (
        <InstallSheet key={activityJobId} apks={[]} devices={devices} defaultSerials={selected}
          jobId={activityJobId} onClose={() => setActivityJobId(null)} />
      )}

      {pending && (
        <InstallSheet
          apks={pending}
          inspectionErrors={inspectionErrors}
          devices={devices}
          defaultSerials={selected}
          onClose={() => {
            setPending(null);
            refresh();
          }}
        />
      )}
    </div>
  );
}

function Placeholder({ title }: { title: string }) {
  return (
    <div className="empty">
      <div className="empty__icon">
        <IconCompass size={22} />
      </div>
      <div className="empty__title">{title} arrives with the catalog</div>
      <div className="empty__text">
        Local device management works fully offline today. The catalog, reviews
        and recommendations come online once the backend is connected.
      </div>
    </div>
  );
}

function SettingsScreen() {
  const { env, refresh } = useEnvironment();
  const { settings, setShowSystemApps } = useSettings();
  const toast = useToast();

  const pickAdb = async () => {
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const result = await open({
        multiple: false,
        filters: [{ name: "adb", extensions: ["exe", ""] }],
      });
      if (typeof result === "string") {
        await api.setAdbPath(result);
        await refresh();
        toast.success("Device tools updated");
      }
    } catch (err) {
      toast.error("Could not set that path", err);
    }
  };

  return (
    <div className="stack-6" style={{ maxWidth: 620 }}>
      <section className="card">
        <div className="card__body stack-4">
          <div className="section-title">Device tools</div>
          <div className="info-grid">
            <span className="dim" style={{ fontSize: "var(--text-sm)" }}>
              Status
            </span>
            <span>
              {env?.adbAvailable ? (
                <span className="badge badge--success">Ready</span>
              ) : (
                <span className="badge badge--danger">Not found</span>
              )}
            </span>
            <span className="dim" style={{ fontSize: "var(--text-sm)" }}>
              Source
            </span>
            <span style={{ fontSize: "var(--text-sm)" }}>
              {env?.adbSource ?? "—"}
            </span>
            <span className="dim" style={{ fontSize: "var(--text-sm)" }}>
              Version
            </span>
            <span style={{ fontSize: "var(--text-sm)" }}>
              {env?.adbVersion ?? "—"}
            </span>
            <span className="dim" style={{ fontSize: "var(--text-sm)" }}>
              Path
            </span>
            <span className="mono truncate" title={env?.adbPath ?? ""}>
              {env?.adbPath ?? "auto-detect"}
            </span>
          </div>
          <div className="row">
            <button className="btn" onClick={() => void pickAdb()}>
              Choose adb…
            </button>
            <button
              className="btn btn--ghost"
              onClick={() => void api.setAdbPath(null).then(refresh)}
            >
              Reset to auto
            </button>
          </div>
        </div>
      </section>

      <section className="card">
        <div className="card__body stack-4">
          <div className="section-title">Library</div>
          <label className="row row--between" style={{ cursor: "pointer" }}>
            <div className="col" style={{ gap: 2 }}>
              <span>Show system apps</span>
              <span className="dim" style={{ fontSize: "var(--text-sm)" }}>
                Include vendor packages that are normally hidden.
              </span>
            </div>
            <input
              type="checkbox"
              checked={settings?.showSystemApps ?? false}
              onChange={(e) => void setShowSystemApps(e.target.checked)}
            />
          </label>
        </div>
      </section>

      <section className="card">
        <div className="card__body stack-3">
          <div className="section-title">Catalog</div>
          <div className="muted" style={{ fontSize: "var(--text-sm)" }}>
            No backend is configured, so VRManager runs in offline mode: devices,
            installed apps and local APK installs all work. The catalog, reviews
            and recommendations appear once a backend URL is set.
          </div>
          <span className="badge">Offline mode</span>
        </div>
      </section>
    </div>
  );
}
