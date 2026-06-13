import { useAppStore } from "../store/appStore";

export function useTaskState() {
  const busy = useAppStore((state) => state.busy);
  const setBusy = useAppStore((state) => state.setBusy);
  const autoRunState = useAppStore((state) => state.autoRunState);
  const setAutoRunState = useAppStore((state) => state.setAutoRunState);
  const autoControlBusy = useAppStore((state) => state.autoControlBusy);
  const setAutoControlBusy = useAppStore((state) => state.setAutoControlBusy);
  const job = useAppStore((state) => state.job);
  const setJob = useAppStore((state) => state.setJob);
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
