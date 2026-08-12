import { AppSettingsView, type SettingsTab } from "../Settings/AppSettings";
import type { AppSettings, ModelProfile } from "../../types";

type SettingsPageProps = {
  settings: AppSettings;
  profiles: ModelProfile[];
  busy: string;
  processing: boolean;
  autoContinueSettingBusy: boolean;
  pausedAutoRun: boolean;
  onBack: () => void;
  onChooseExportDir: () => void;
  onClearExportDir: () => void;
  onToggleReview: () => void;
  onReviewProfileChange: (profileId: string) => void;
  onAnalysisProfileChange: (profileId: string) => void;
  onBatchSizeChange: (value: 10 | 30 | 50 | 100) => void;
  onParallelismChange: (value: 1 | 3 | 6 | 10 | 25 | 50) => void;
  onToggleAutoContinue: () => void;
  onDeleteLocalData: () => void;
  activeTab: SettingsTab;
  onTabChange: (tab: SettingsTab) => void;
};

export function SettingsPage(props: SettingsPageProps) {
  return (
    <AppSettingsView
      settings={props.settings}
      profiles={props.profiles}
      busy={props.busy}
      processing={props.processing}
      autoContinueSettingBusy={props.autoContinueSettingBusy}
      allowPausedTaskAdjustments={props.pausedAutoRun}
      onBack={props.onBack}
      onChooseExportDir={props.onChooseExportDir}
      onClearExportDir={props.onClearExportDir}
      onToggleReview={props.onToggleReview}
      onReviewProfileChange={props.onReviewProfileChange}
      onAnalysisProfileChange={props.onAnalysisProfileChange}
      onBatchSizeChange={props.onBatchSizeChange}
      onParallelismChange={props.onParallelismChange}
      onToggleAutoContinue={props.onToggleAutoContinue}
      onDeleteLocalData={props.onDeleteLocalData}
      activeTab={props.activeTab}
      onTabChange={props.onTabChange}
    />
  );
}
