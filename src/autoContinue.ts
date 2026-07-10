import type { AutoRunPauseKind, AutoRunRecovery } from "./types";

const automaticPauseKinds = new Set<AutoRunPauseKind>([
  "rate_limit",
  "network",
  "temporary_gateway",
  "model_format",
  "content_filter"
]);

const retryDelays: Record<Exclude<AutoRunPauseKind, "user" | "interrupted" | "unknown" | "">, number[]> = {
  rate_limit: [300, 600, 900],
  network: [10, 30, 60, 120, 300],
  temporary_gateway: [10, 30, 60, 120, 300],
  model_format: [30, 60, 120, 300, 600],
  content_filter: [30, 60, 120, 300, 600]
};

export function isAutomaticPauseKind(kind: AutoRunPauseKind | undefined): boolean {
  return kind !== undefined && automaticPauseKinds.has(kind);
}

export function autoContinueDelaySeconds(kind: AutoRunPauseKind, attemptIndex: number): number {
  if (!isAutomaticPauseKind(kind)) return 0;
  const delays = retryDelays[kind as keyof typeof retryDelays];
  return delays[Math.min(Math.max(0, attemptIndex), delays.length - 1)];
}

export function autoContinueProgressFingerprint(recovery: AutoRunRecovery): string {
  const summary = recovery.summary;
  return [
    recovery.next_batch_index,
    recovery.batch_index ?? "",
    recovery.phase ?? "",
    summary?.staged_chapters ?? 0,
    summary?.pending_chapters ?? 0
  ].join(":");
}

export function autoContinuePauseKey(recovery: AutoRunRecovery): string {
  return recovery.job?.id
    ?? [recovery.novel_id, recovery.pause_kind, recovery.pause_reason, autoContinueProgressFingerprint(recovery)].join(":");
}
