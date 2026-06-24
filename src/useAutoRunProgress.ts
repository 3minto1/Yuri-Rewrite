import { useEffect, useRef } from "react";
import { listenAppEvent } from "./platform/runtime";
import type { Job } from "./types";

export type AutoRunProgress = Job;

export function useAutoRunProgress(
  novelId: string | null,
  onProgress: (progress: AutoRunProgress) => void
) {
  const callbackRef = useRef(onProgress);
  const activeJobIdRef = useRef("");
  const terminalJobIdRef = useRef("");

  useEffect(() => {
    callbackRef.current = onProgress;
  }, [onProgress]);

  useEffect(() => {
    activeJobIdRef.current = "";
    terminalJobIdRef.current = "";
  }, [novelId]);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void listenAppEvent<AutoRunProgress>("job-progress", (event) => {
      const progress = event.payload;
      if (
        !novelId
        || !["auto", "auto_batch"].includes(progress.job_type)
        || progress.novel_id !== novelId
      ) return;
      if (terminalJobIdRef.current === progress.id) return;
      if (activeJobIdRef.current && activeJobIdRef.current !== progress.id) return;
      activeJobIdRef.current = progress.id;
      callbackRef.current(progress);
      if (["completed", "failed", "terminated", "paused"].includes(progress.status)) {
        terminalJobIdRef.current = progress.id;
        activeJobIdRef.current = "";
      }
    }).then((handler) => {
      if (cancelled) handler();
      else unlisten = handler;
    });
    return () => {
      cancelled = true;
      if (unlisten) unlisten();
    };
  }, [novelId]);
}
