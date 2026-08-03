import { Loader2, ShieldCheck, X } from "lucide-react";
import { useMemo, useState } from "react";
import type { NameMappingConsistencyReport } from "../../types";
import { Modal } from "../common/Modal";

type NameMappingCleanupDialogProps = {
  busy: boolean;
  report: NameMappingConsistencyReport;
  onCancel: () => void;
  onConfirm: (keepAsManualSources: string[]) => void;
};

export function NameMappingCleanupDialog({
  busy,
  report,
  onCancel,
  onConfirm
}: NameMappingCleanupDialogProps) {
  const [keptSources, setKeptSources] = useState<Set<string>>(() => new Set());
  const legacySources = useMemo(
    () => report.legacy_unmanaged.map((entry) => entry.source),
    [report.legacy_unmanaged]
  );

  function toggleSource(source: string, keep: boolean) {
    setKeptSources((current) => {
      const next = new Set(current);
      if (keep) next.add(source);
      else next.delete(source);
      return next;
    });
  }

  return (
    <Modal
      className="settings-dialog name-mapping-cleanup-dialog"
      labelledBy="name-mapping-cleanup-title"
      onRequestClose={busy ? undefined : onCancel}
    >
      <header className="dialog-titlebar">
        <h2 id="name-mapping-cleanup-title">检查旧版姓名映射</h2>
        <button
          className="dialog-close"
          type="button"
          aria-label="关闭旧版姓名映射检查"
          title="关闭"
          onClick={onCancel}
          disabled={busy}
        >
          <X size={16} />
        </button>
      </header>
      <div className="dialog-body name-mapping-cleanup-body">
        <p>
          这些映射来自旧版数据，目前不在小说设定中。默认删除；如果是你手动补充的映射，请勾选保留。
        </p>
        <div className="name-mapping-cleanup-list">
          {report.legacy_unmanaged.map((entry) => (
            <label key={entry.source} className="name-mapping-cleanup-row">
              <input
                type="checkbox"
                checked={keptSources.has(entry.source)}
                onChange={(event) => toggleSource(entry.source, event.target.checked)}
                disabled={busy}
              />
              <span className="name-mapping-cleanup-pair">
                <strong>{entry.source}</strong>
                <span aria-hidden="true">→</span>
                <strong>{entry.target}</strong>
              </span>
              <span className="name-mapping-cleanup-action">
                {keptSources.has(entry.source) ? "保留为手动映射" : "删除旧映射"}
              </span>
            </label>
          ))}
        </div>
        <p className="field-hint">
          此操作不会修改原始 TXT 或已有改写稿。删除后如需更新旧稿，请重新改写受影响批次。
        </p>
      </div>
      <footer className="dialog-actions">
        <button type="button" onClick={onCancel} disabled={busy}>取消</button>
        <button
          type="button"
          onClick={() => onConfirm(legacySources.filter((source) => keptSources.has(source)))}
          disabled={busy}
        >
          {busy ? <Loader2 className="spin" size={16} /> : <ShieldCheck size={16} />}
          应用处理
        </button>
      </footer>
    </Modal>
  );
}
