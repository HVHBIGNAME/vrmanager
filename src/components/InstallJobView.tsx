import type { DeviceSnapshot } from "../lib/api";
import { stageLabel, type InstallJobSnapshot, type JobStatus } from "../lib/events";
import { retryableItems, useInstallActivity } from "../lib/installActivity";

const jobLabel: Record<JobStatus, string> = {
  queued: "Queued", running: "In progress", completed: "Completed", failed: "Failed", cancelled: "Cancelled",
};

export function ActivityIssues() {
  const { activity, state } = useInstallActivity();
  const issues = Object.entries(state.issues);
  if (!issues.length) return null;
  return <div className="stack-2">
    {issues.map(([key, issue]) => <div key={key} role="alert" className="notice notice--danger">
      <strong>{issue.title}</strong><div>{issue.error.message}</div>
      {issue.error.details && <details className="details"><summary>Technical details</summary>
        <div className="details__content">{issue.error.details}</div></details>}
    </div>)}
    <button className="btn btn--sm" disabled={state.pending.list} onClick={() => void activity.reload()}>Reload activity</button>
  </div>;
}

export function InstallJobView({ jobId, devices, onRetried }: {
  jobId: string; devices: DeviceSnapshot[]; onRetried?: (jobId: string) => void;
}) {
  const { activity, state } = useInstallActivity();
  const job = state.jobs[jobId];
  if (!job) return <div className="notice stack-2">
    <div>Installation accepted. Status is not available yet.</div>
    <div className="mono">{jobId}</div>
    <button className="btn btn--sm" disabled={state.pending[`fetch:${jobId}`]}
      onClick={() => void activity.fetch(jobId)}>Reload status</button>
    <JobActions jobId={jobId} onRetried={onRetried} />
  </div>;
  return <div className="stack-3">
    <JobSummary job={job} />
    <JobActions jobId={jobId} onRetried={onRetried} />
    <ul className="stack-2" style={{ listStyle: "none", margin: 0, padding: 0 }}>
      {job.items.map((item) => {
        const name = item.applicationName || item.apkPath.split(/[\\/]/).pop() || item.apkPath;
        const device = devices.find((d) => d.serial === item.serial)?.displayName || item.serial;
        const label = item.status === "succeeded" ? "Installed" : item.status === "failed" ? "Failed"
          : item.status === "cancelled" ? "Cancelled" : item.status === "queued" ? "Queued" : stageLabel[item.stage];
        return <li key={item.itemId} className="target" aria-label={`${name} on ${device}`}>
          <div className="row row--between">
            <strong className="truncate" title={name}>{name}</strong>
            <span className={`badge ${item.status === "succeeded" ? "badge--success" : item.status === "failed" ? "badge--danger" : ""}`}>{label}</span>
          </div>
          <div className="muted">{device} · <span className="mono">{item.serial}</span></div>
          <div className="mono truncate" title={item.apkPath}>{item.apkPath}</div>
          {item.package && <div className="dim mono">{item.package}</div>}
          {item.status === "succeeded" && item.package && <button className="btn btn--sm"
            aria-label={`Launch ${name} on ${device}`} disabled={state.pending[`launch:${jobId}:${item.itemId}`]}
            onClick={() => void activity.launch(jobId, item.itemId)}>
            {state.pending[`launch:${jobId}:${item.itemId}`] ? "Launching…" : "Launch"}
          </button>}
          {item.status === "running" && <div className="stack-2">
            {item.percent != null && <div className="muted mono">{item.percent}%</div>}
            <div className="progress" role="progressbar" aria-label={`${name} on ${device}`}
              aria-valuemin={0} aria-valuemax={100} aria-valuenow={item.percent ?? undefined}>
              <div className={`progress__bar ${item.percent == null ? "progress__bar--indeterminate" : ""}`}
                style={item.percent != null ? { width: `${item.percent}%` } : undefined} />
            </div>
          </div>}
          {item.error && <div className="notice notice--danger" role="alert">
            {item.error.message}
            {item.error.details && <details className="details"><summary>Technical details</summary>
              <div className="details__content">{item.error.details}</div></details>}
          </div>}
        </li>;
      })}
    </ul>
  </div>;
}

export function JobSummary({ job }: { job: InstallJobSnapshot }) {
  return <div className="row row--between" aria-live="polite">
    <span className={`badge ${job.status === "completed" ? "badge--success" : job.status === "failed" ? "badge--danger" : ""}`}>{jobLabel[job.status]}</span>
    <span className="muted mono">{job.items.filter((item) => item.status === "succeeded").length} / {job.items.length} installed</span>
  </div>;
}

function JobActions({ jobId, onRetried }: { jobId: string; onRetried?: (jobId: string) => void }) {
  const { activity, state } = useInstallActivity();
  const job = state.jobs[jobId];
  const active = !job || job.status === "queued" || job.status === "running";
  const eligible = retryableItems(job);
  const retry = async () => {
    const id = await activity.retry(jobId);
    if (id) onRetried?.(id);
  };
  return <div className="row">
    {active && <button className="btn btn--sm btn--danger" aria-label="Cancel installation"
      disabled={state.pending[`cancel:${jobId}`]} onClick={() => void activity.cancel(jobId)}>
      {state.pending[`cancel:${jobId}`] ? "Cancelling…" : "Cancel installation"}
    </button>}
    {eligible.length > 0 && <button className="btn btn--sm" aria-label="Retry eligible items"
      title="Only retryable failed and cancelled targets are retried."
      disabled={state.pending[`retry:${jobId}`]} onClick={() => void retry()}>
      {state.pending[`retry:${jobId}`] ? "Retrying…" : `Retry eligible (${eligible.length})`}
    </button>}
  </div>;
}
