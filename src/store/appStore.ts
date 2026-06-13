import { create } from "zustand";
import type { AutoRunState, Job, ModelProfile, Novel, NovelDetail } from "../types";

type AppStore = {
  novels: Novel[];
  detail: NovelDetail | null;
  selectedChapterId: string;
  selectedBatchId: string;
  profiles: ModelProfile[];
  selectedProfileId: string;
  busy: string;
  autoRunState: AutoRunState;
  autoControlBusy: boolean;
  job: Job | null;
  setNovels: (novels: Novel[]) => void;
  setDetail: (detail: NovelDetail | null) => void;
  setSelectedChapterId: (id: string) => void;
  setSelectedBatchId: (id: string) => void;
  setProfiles: (profiles: ModelProfile[]) => void;
  setSelectedProfileId: (id: string) => void;
  setBusy: (busy: string) => void;
  setAutoRunState: (state: AutoRunState) => void;
  setAutoControlBusy: (busy: boolean) => void;
  setJob: (job: Job | null) => void;
  reset: () => void;
};

const initialState = {
  novels: [],
  detail: null,
  selectedChapterId: "",
  selectedBatchId: "",
  profiles: [],
  selectedProfileId: "",
  busy: "",
  autoRunState: "idle" as AutoRunState,
  autoControlBusy: false,
  job: null
};

export const useAppStore = create<AppStore>((set) => ({
  ...initialState,
  setNovels: (novels) => set({ novels }),
  setDetail: (detail) => set({ detail }),
  setSelectedChapterId: (selectedChapterId) => set({ selectedChapterId }),
  setSelectedBatchId: (selectedBatchId) => set({ selectedBatchId }),
  setProfiles: (profiles) => set({ profiles }),
  setSelectedProfileId: (selectedProfileId) => set({ selectedProfileId }),
  setBusy: (busy) => set({ busy }),
  setAutoRunState: (autoRunState) => set({ autoRunState }),
  setAutoControlBusy: (autoControlBusy) => set({ autoControlBusy }),
  setJob: (job) => set({ job }),
  reset: () => set(initialState)
}));
