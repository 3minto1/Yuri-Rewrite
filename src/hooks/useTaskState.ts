import { useState } from "react";
import type { Job } from "../types";

export type AutoRunState = "idle" | "running" | "paused" | "stopping";

export function useTaskState() {
  const [busy, setBusy] = useState("");
  const [autoRunState, setAutoRunState] = useState<AutoRunState>("idle");
  const [autoControlBusy, setAutoControlBusy] = useState(false);
  const [job, setJob] = useState<Job | null>(null);
  const processingTaskActive =
    ["analysis", "rewrite", "auto-batch", "auto"].includes(busy) || autoRunState !== "idle";

  return {
    busy,
    setBusy,
    autoRunState,
    setAutoRunState,
    autoControlBusy,
    setAutoControlBusy,
    job,
    setJob,
    processingTaskActive
  };
}
