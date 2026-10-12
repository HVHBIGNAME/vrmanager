import { createContext, useContext, useEffect, useState, useSyncExternalStore, type ReactNode } from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { api, toUiError, type ApkInspectionBatch, type UiError } from "./api";
import { onInstallJobChanged, type InstallJobSnapshot } from "./events";

export interface ActivityIssue {
  title: string;
  error: UiError;
}
export interface ActivityState {
  jobs: Record<string, InstallJobSnapshot | undefined>;
  previews: Record<string, ApkInspectionBatch>;
  issues: Record<string, ActivityIssue>;
  pending: Record<string, boolean>;
  connected: boolean;
}

export function retryableItems(job: InstallJobSnapshot | undefined) {
  if (!job || (job.status !== "failed" && job.status !== "cancelled")) return [];
  return job.items.filter((item) => item.status === "cancelled" || (item.status === "failed" && item.error?.retryable === true));
}

/** Rust owns execution. This store only reconciles whole, revisioned snapshots. */
export class InstallActivity {
  private state: ActivityState = { jobs: {}, previews: {}, issues: {}, pending: {}, connected: false };
  private observers = new Set<() => void>();
  private subscription: Promise<UnlistenFn> | null = null;
  private mounted = false;
  private generation = 0;
  private listRequest = 0;

  getSnapshot = () => this.state;
  subscribe = (observer: () => void) => {
    this.observers.add(observer);
    return () => { this.observers.delete(observer); };
  };
  private update(patch: Partial<ActivityState>) {
    this.state = { ...this.state, ...patch };
    this.observers.forEach((observer) => observer());
  }
  private clearIssue(key: string) {
    const issues = { ...this.state.issues };
    delete issues[key];
    this.update({ issues });
  }
  private issue(key: string, title: string, error: unknown) {
    this.update({ issues: { ...this.state.issues, [key]: { title, error: toUiError(error) } } });
  }
  private accept(snapshot: InstallJobSnapshot) {
    const previous = this.state.jobs[snapshot.jobId];
    if (previous && snapshot.revision <= previous.revision) return;
    this.update({ jobs: { ...this.state.jobs, [snapshot.jobId]: snapshot } });
  }
  private remember(jobId: string) {
    if (!(jobId in this.state.jobs)) this.update({ jobs: { ...this.state.jobs, [jobId]: undefined } });
  }
  rememberPreview(jobId: string, preview: ApkInspectionBatch) {
    this.update({ previews: { ...this.state.previews, [jobId]: preview } });
  }

  connect = () => {
    this.mounted = true;
    const generation = ++this.generation;
    void this.reload();
    return () => {
      if (generation !== this.generation) return;
      this.mounted = false;
      ++this.generation;
      const subscription = this.subscription;
      this.subscription = null;
      // Also releases a listener whose registration resolves AFTER StrictMode cleanup.
      void subscription?.then((unlisten) => unlisten(), () => {});
      this.update({ connected: false, pending: {} });
    };
  };

  private ensureSubscription(): Promise<UnlistenFn> {
    if (this.subscription) return this.subscription;
    const generation = this.generation;
    const live = () => this.mounted && generation === this.generation;
    this.clearIssue("subscription");
    this.subscription = onInstallJobChanged((snapshot) => {
      if (live()) this.accept(snapshot);
    }).then((unlisten) => {
      if (live()) this.update({ connected: true });
      return unlisten;
    }).catch((err: unknown) => {
      if (live()) {
        this.subscription = null;
        this.update({ connected: false });
        this.issue("subscription", "Live installation updates unavailable", err);
      }
      throw err;
    });
    return this.subscription;
  }

  reload = async () => {
    const generation = this.generation;
    const request = ++this.listRequest;
    const live = () => this.mounted && generation === this.generation && request === this.listRequest;
    this.update({ pending: { ...this.state.pending, list: true } });
    this.clearIssue("list");
    try {
      // A failed listener must be visible, but does not prevent reading history.
      try { await this.ensureSubscription(); } catch { /* reported separately */ }
      if (!live()) return;
      const jobs = await api.listInstallJobs();
      if (live()) jobs.forEach((snapshot) => {
        this.accept(snapshot);
        this.clearIssue(`fetch:${snapshot.jobId}`);
      });
    } catch (err) {
      if (live()) this.issue("list", "Could not load installation activity", err);
    } finally {
      if (live()) this.update({ pending: { ...this.state.pending, list: false } });
    }
  };

  private async perform<T>(key: string, title: string, action: (live: () => boolean) => Promise<T>): Promise<T | undefined> {
    if (!this.mounted || this.state.pending[key]) return;
    const generation = this.generation;
    const live = () => this.mounted && generation === this.generation;
    this.clearIssue(key);
    this.update({ pending: { ...this.state.pending, [key]: true } });
    try {
      return await action(live);
    } catch (err) {
      if (this.mounted && generation === this.generation) this.issue(key, title, err);
    } finally {
      if (this.mounted && generation === this.generation) {
        this.update({ pending: { ...this.state.pending, [key]: false } });
      }
    }
  }

  fetch = (jobId: string) => this.perform(`fetch:${jobId}`, "Could not read installation status", async (live) => {
    const snapshot = await api.getInstallJob(jobId);
    if (!live()) return;
    if (snapshot.jobId !== jobId) throw "The core returned a different installation job.";
    this.accept(snapshot);
    return snapshot;
  });

  start = (serials: string[], apkPaths: string[]) => this.perform("start", "Could not start installation", async (live) => {
    // Do not invoke until registration has resolved: completion can precede the reply.
    await this.ensureSubscription();
    if (!live()) return;
    const { jobId } = await api.startInstallQueue(serials, apkPaths);
    if (!live()) return;
    this.remember(jobId);
    await this.fetch(jobId);
    // A fetch failure is NOT a start failure; keep the accepted id for recovery.
    return live() ? jobId : undefined;
  });

  cancel = (jobId: string) => this.perform(`cancel:${jobId}`, "Could not cancel installation", async (live) => {
    await api.cancelJob(jobId);
    if (live()) await this.fetch(jobId);
  });

  retry = (jobId: string) => this.perform(`retry:${jobId}`, "Could not retry installation", async (live) => {
    if (!retryableItems(this.state.jobs[jobId]).length) {
      throw { kind: "invalidState", message: "No eligible targets to retry in this job.", retryable: false };
    }
    await this.ensureSubscription();
    if (!live()) return;
    // The core chooses original eligible path/serial pairs; never rebuild a cartesian queue here.
    const result = await api.retryInstallJob(jobId);
    if (!live()) return;
    this.remember(result.jobId);
    const preview = this.state.previews[jobId];
    if (preview) this.rememberPreview(result.jobId, preview);
    await this.fetch(result.jobId);
    return live() ? result.jobId : undefined;
  });

  launch = (jobId: string, itemId: string) => this.perform(`launch:${jobId}:${itemId}`, "Could not launch app", async () => {
    const item = this.state.jobs[jobId]?.items.find((candidate) => candidate.itemId === itemId);
    if (item?.status !== "succeeded" || !item.package) {
      throw { kind: "invalidState", message: "Only a verified installed package can be launched.", retryable: false };
    }
    await api.launchApp(item.serial, item.package);
  });
}

const ActivityContext = createContext<{ activity: InstallActivity; state: ActivityState } | null>(null);
export function InstallActivityProvider({ children }: { children: ReactNode }) {
  const [activity] = useState(() => new InstallActivity());
  const state = useSyncExternalStore(activity.subscribe, activity.getSnapshot);
  useEffect(() => activity.connect(), [activity]);
  return <ActivityContext.Provider value={{ activity, state }}>{children}</ActivityContext.Provider>;
}
export function useInstallActivity() {
  const context = useContext(ActivityContext);
  if (!context) throw new Error("InstallActivityProvider is required");
  return context;
}
