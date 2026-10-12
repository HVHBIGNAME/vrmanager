import { useEffect } from "react";
import type { DeviceSnapshot } from "../lib/api";
import { useInstallActivity } from "../lib/installActivity";
import { ActivityIssues, InstallJobView } from "../components/InstallJobView";

/** Local installation history, never a simulated remote download queue. */
export function ActivityScreen({ devices, onOpen }: { devices: DeviceSnapshot[]; onOpen: (jobId: string) => void }) {
  const { activity, state } = useInstallActivity();
  useEffect(() => { void activity.reload(); }, [activity]);
  const ids = Object.keys(state.jobs).sort((a, b) =>
    (state.jobs[b]?.createdAtMs ?? 0) - (state.jobs[a]?.createdAtMs ?? 0) || a.localeCompare(b));
  return <section className="stack-4" aria-label="Local installation activity">
    <div className="row row--between">
      <div>
        <h1 className="h3">Local installation activity</h1>
        <div className="muted">APK installs in this app session. Remote downloads are not implemented.</div>
      </div>
      <button className="btn btn--sm" disabled={state.pending.list} onClick={() => void activity.reload()}>Refresh activity</button>
    </div>
    <ActivityIssues />
    {state.pending.list && <div role="status" className="notice">Loading installation activity…</div>}
    {!state.pending.list && !ids.length && !state.issues.list && <div className="empty">
      <div className="empty__title">No local installations yet</div>
      <div className="empty__text">Use Install APK or drop APK files to install them on a connected headset.</div>
    </div>}
    {ids.map((jobId) => <article key={jobId} className="card" aria-label={`Installation ${jobId}`}>
      <div className="card__body stack-3">
        <div className="row row--between">
          <div className="col" style={{ minWidth: 0 }}>
            <strong className="section-title">Installation</strong>
            <span className="mono truncate" title={jobId}>{jobId}</span>
            {state.jobs[jobId] && <time className="muted" dateTime={new Date(state.jobs[jobId]!.updatedAtMs).toISOString()}>
              Updated {new Date(state.jobs[jobId]!.updatedAtMs).toLocaleString()}
            </time>}
          </div>
          <button className="btn btn--sm" aria-label={`Open details for ${jobId}`} onClick={() => onOpen(jobId)}>Open details</button>
        </div>
        <InstallJobView jobId={jobId} devices={devices} onRetried={onOpen} />
      </div>
    </article>)}
    {ids.length > 0 && <p className="muted">Closing details keeps the job running. History is retained for this app session, not after quitting VRManager.</p>}
  </section>;
}
