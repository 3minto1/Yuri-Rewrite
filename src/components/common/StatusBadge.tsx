import { memo } from "react";

export type StatusTone = "success" | "progress" | "warning" | "danger" | "neutral";

const STATUS_TONES: Record<string, StatusTone> = {
  completed: "success",
  success: "success",
  ok: "success",
  running: "progress",
  processing: "progress",
  pausing: "warning",
  paused: "warning",
  warning: "warning",
  failed: "danger",
  error: "danger",
  pending: "neutral",
  imported: "neutral",
  terminated: "neutral",
  cancelled: "neutral",
  canceled: "neutral"
};

export function getStatusTone(status?: string | null): StatusTone {
  return STATUS_TONES[status?.trim().toLowerCase() ?? ""] ?? "neutral";
}

type StatusBadgeProps = {
  status?: string | null;
  label: string;
  showDot?: boolean;
  className?: string;
};

export const StatusBadge = memo(function StatusBadge({
  status,
  label,
  showDot = true,
  className = ""
}: StatusBadgeProps) {
  const tone = getStatusTone(status);
  return (
    <span className={`status-badge status-${tone}${className ? ` ${className}` : ""}`}>
      {showDot && <span className="status-badge-dot" aria-hidden="true" />}
      <span>{label}</span>
    </span>
  );
});
