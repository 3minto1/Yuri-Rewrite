import { useCallback, useEffect, useState } from "react";
import type { Dispatch, SetStateAction } from "react";
import type { UpdateCheckResult } from "../types";

export function useNotice(
  setPendingUpdate: Dispatch<SetStateAction<UpdateCheckResult | null>>
) {
  const [notice, setNotice] = useState("");
  const [duration, setDuration] = useState(5000);

  useEffect(() => {
    if (!notice) return undefined;
    const timer = window.setTimeout(() => setNotice(""), duration);
    return () => window.clearTimeout(timer);
  }, [duration, notice]);

  const showNotice = useCallback(
    (message: string, nextDuration = 5000, keepPendingUpdate = false) => {
      if (!keepPendingUpdate) setPendingUpdate(null);
      setDuration(nextDuration);
      setNotice(message);
    },
    [setPendingUpdate]
  );

  return { notice, setNotice, showNotice };
}
