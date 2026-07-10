import { Loader2, Plus, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { invokeCommand as invoke } from "../../tauriApi";
import type { ModelProfile, RewriteAbEstimate, RewriteAbRunDetail, RewriteAbRunSummary } from "../../types";
import { Modal } from "../common/Modal";

type Props = {
  novelId: string;
  batchId: string;
  batchLabel: string;
  profiles: ModelProfile[];
  defaultProfileId: string;
  onCancel: () => void;
  onOpenRun: (runId: string) => void;
  onStarted: (run: RewriteAbRunDetail) => void;
  onNotice: (message: string) => void;
  onTaskBusyChange: (busy: boolean) => void;
};

function formatSeconds(value?: number | null) {
  if (!value) return "暂无历史数据";
  if (value < 60) return `约 ${Math.ceil(value)} 秒`;
  return `约 ${Math.ceil(value / 60)} 分钟`;
}

export function RewriteAbStartDialog(props: Props) {
  const { novelId, batchId, batchLabel, profiles, defaultProfileId, onCancel, onOpenRun, onStarted, onNotice, onTaskBusyChange } = props;
  const fallbackB = profiles.find((profile) => profile.id !== defaultProfileId)?.id ?? "";
  const [profileIds, setProfileIds] = useState([defaultProfileId, fallbackB]);
  const [reviewEnabled, setReviewEnabled] = useState(false);
  const [estimate, setEstimate] = useState<RewriteAbEstimate | null>(null);
  const [runs, setRuns] = useState<RewriteAbRunSummary[]>([]);
  const [loadingEstimate, setLoadingEstimate] = useState(false);
  const [starting, setStarting] = useState(false);
  const closeRef = useRef<HTMLButtonElement>(null);
  const selectedIds = profileIds.filter(Boolean);
  const selectionValid = selectedIds.length >= 2
    && selectedIds.length <= 3
    && new Set(selectedIds).size === selectedIds.length;
  const matchingRun = useMemo(
    () => runs.find((run) => run.id === estimate?.existing_run_id),
    [estimate?.existing_run_id, runs]
  );

  useEffect(() => {
    void invoke("list_rewrite_ab_runs", { novelId })
      .then(setRuns)
      .catch((error) => onNotice(String(error)));
  }, [novelId, onNotice]);

  useEffect(() => {
    if (!selectionValid) {
      setEstimate(null);
      return undefined;
    }
    let cancelled = false;
    setLoadingEstimate(true);
    const timer = window.setTimeout(() => {
      void invoke("estimate_rewrite_ab", { novelId, batchId, profileIds: selectedIds, reviewEnabled })
        .then((value) => { if (!cancelled) setEstimate(value); })
        .catch((error) => { if (!cancelled) { setEstimate(null); onNotice(String(error)); } })
        .finally(() => { if (!cancelled) setLoadingEstimate(false); });
    }, 150);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [batchId, novelId, onNotice, profileIds, reviewEnabled, selectionValid]);

  function updateProfile(index: number, value: string) {
    setProfileIds((current) => current.map((id, offset) => offset === index ? value : id));
  }

  async function start() {
    if (!selectionValid) return;
    const existingRunId = estimate?.existing_run_id ?? "";
    if (existingRunId) {
      const existingStatus = matchingRun?.status ? `（状态 ${matchingRun.status}）` : "";
      if (!window.confirm(`相同章节范围已有一次 A/B 实验${existingStatus}。新实验会替换它，是否继续？`)) return;
    }
    setStarting(true);
    onTaskBusyChange(true);
    onCancel();
    let finished = false;
    let openedRunningRun = false;
    const startPromise = invoke("start_rewrite_ab", {
      novelId,
      batchId,
      profileIds: selectedIds,
      reviewEnabled,
      replaceRunId: existingRunId || undefined
    });
    const pollForRunningRun = async () => {
      while (!finished && !openedRunningRun) {
        await new Promise((resolve) => window.setTimeout(resolve, 300));
        if (finished) break;
        try {
          const currentRuns = await invoke("list_rewrite_ab_runs", { novelId });
          const running = currentRuns.find((run) => run.batch_id === batchId && run.status === "running" && run.id !== existingRunId);
          if (running) {
            openedRunningRun = true;
            onOpenRun(running.id);
          }
        } catch {
          // The start command remains authoritative; a transient list failure is harmless.
        }
      }
    };
    void pollForRunningRun();
    try {
      const run = await startPromise;
      finished = true;
      onStarted(run);
    } catch (error) {
      finished = true;
      onNotice(String(error));
    } finally {
      finished = true;
      onTaskBusyChange(false);
    }
  }

  return (
    <Modal className="settings-dialog rewrite-ab-start-dialog" labelledBy="rewrite-ab-start-title" onRequestClose={starting ? undefined : onCancel} initialFocusRef={closeRef}>
      <header className="dialog-titlebar">
        <div>
          <h2 id="rewrite-ab-start-title">A/B 改写当前批次</h2>
          <p>{batchLabel}</p>
        </div>
        <button ref={closeRef} className="dialog-close" type="button" aria-label="关闭 A/B 改写" onClick={onCancel} disabled={starting}><X size={16} /></button>
      </header>
      <div className="dialog-body rewrite-ab-start-body">
        <p className="rewrite-ab-cost-warning">每个模型会独立改写同一批次，预计产生普通改写的 {selectedIds.length || 2} 倍调用量。候选不会自动覆盖正式改写稿。</p>
        <fieldset className="rewrite-ab-model-fields">
          <legend>候选模型</legend>
          {profileIds.map((profileId, index) => (
            <label key={index}>
              模型 {String.fromCharCode(65 + index)}
              <span className="rewrite-ab-model-row">
                <select aria-label={`模型 ${String.fromCharCode(65 + index)}`} value={profileId} onChange={(event) => updateProfile(index, event.target.value)}>
                  <option value="">请选择模型</option>
                  {profiles.map((profile) => <option key={profile.id} value={profile.id}>{profile.name} · {profile.model}</option>)}
                </select>
                {index === 2 && <button type="button" aria-label="移除模型 C" onClick={() => setProfileIds((current) => current.slice(0, 2))}><X size={15} /></button>}
              </span>
            </label>
          ))}
          {profileIds.length === 2 && <button type="button" className="rewrite-ab-add-model" onClick={() => setProfileIds((current) => [...current, profiles.find((profile) => !current.includes(profile.id))?.id ?? ""])}><Plus size={15} />增加模型 C</button>}
        </fieldset>
        {!selectionValid && <p className="field-error" role="alert">请选择 2–3 个不同且已保存的模型。</p>}
        <label className="checkbox-row rewrite-ab-review-option">
          <input type="checkbox" checked={reviewEnabled} onChange={(event) => setReviewEnabled(event.target.checked)} />
          分别复检每个候选（会增加请求数，不会自动选稿）
        </label>
        <section className="rewrite-ab-estimate" aria-live="polite">
          <strong>任务预估</strong>
          {loadingEstimate ? <span><Loader2 className="spin" size={15} />正在测算…</span> : estimate ? (
            <span>{estimate.chapter_count} 章 · {estimate.model_count} 个模型 · 约 {estimate.estimated_requests} 次请求 · {formatSeconds(estimate.estimated_seconds)}</span>
          ) : <span>模型选择有效后显示。</span>}
        </section>
        {runs.length > 0 && (
          <section className="rewrite-ab-recent-runs">
            <strong>已有实验</strong>
            {runs.map((run) => (
              <button type="button" key={run.id} onClick={() => onOpenRun(run.id)}>
                <span>{run.batch_label}</span><small>{run.status} · {run.completed_candidates}/{run.total_candidates}</small>
              </button>
            ))}
          </section>
        )}
      </div>
      <footer className="dialog-actions">
        <button type="button" onClick={onCancel} disabled={starting}>取消</button>
        <button type="button" className="dialog-primary" onClick={() => void start()} disabled={!selectionValid || loadingEstimate || starting}>
          {starting ? <Loader2 className="spin" size={16} /> : null}开始 A/B 改写
        </button>
      </footer>
    </Modal>
  );
}
