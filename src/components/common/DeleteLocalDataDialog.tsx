import { Loader2, Trash2, X } from "lucide-react";
import { useRef, useState } from "react";
import { Modal } from "./Modal";

const confirmationPhrase = "删除全部本地数据";

type DeleteLocalDataDialogProps = {
  busy: boolean;
  onCancel: () => void;
  onConfirm: (confirmationPhrase: string) => void;
};

export function DeleteLocalDataDialog({ busy, onCancel, onConfirm }: DeleteLocalDataDialogProps) {
  const [confirmation, setConfirmation] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);
  const confirmed = confirmation.trim() === confirmationPhrase;
  return (
    <Modal
      className="settings-dialog delete-local-data-dialog"
      labelledBy="delete-local-data-dialog-title"
      onRequestClose={busy ? undefined : onCancel}
      initialFocusRef={inputRef}
    >
      <header className="dialog-titlebar">
        <h2 id="delete-local-data-dialog-title">永久删除本地数据</h2>
        <button
          className="dialog-close"
          type="button"
          aria-label="关闭删除本地数据确认框"
          title="关闭"
          onClick={onCancel}
          disabled={busy}
        >
          <X size={16} />
        </button>
      </header>
      <div className="dialog-body delete-local-data-dialog-body">
        <p><strong>此操作不可撤销。</strong>请先确认重要内容已经导出或备份。</p>
        <div className="delete-scope delete-scope-danger">
          <strong>将永久删除：</strong>
          <ul>
            <li>全部小说、章节、分析结果、改写稿、设定和一致性资产</li>
            <li>全部模型配置及保存的 API Key</li>
            <li>AI 日志、Token 统计、任务记录、检查点和内部批次文件</li>
            <li>更新缓存以及应用生成的错误、审查警告日志</li>
          </ul>
        </div>
        <div className="delete-scope delete-scope-retained">
          <strong>不会删除：</strong>
          <ul>
            <li>最初导入的原始 TXT 和全部已导出 TXT</li>
            <li>程序文件、主题、窗口位置和快速上手已读状态</li>
          </ul>
        </div>
        <label className="field delete-confirmation-field">
          <span>输入“{confirmationPhrase}”以继续</span>
          <input
            ref={inputRef}
            aria-label="输入删除本地数据确认短语"
            autoComplete="off"
            value={confirmation}
            onChange={(event) => setConfirmation(event.target.value)}
            disabled={busy}
          />
        </label>
      </div>
      <footer className="dialog-actions">
        <button type="button" onClick={onCancel} disabled={busy}>取消</button>
        <button
          className="dialog-danger"
          type="button"
          onClick={() => onConfirm(confirmation)}
          disabled={busy || !confirmed}
        >
          {busy ? <Loader2 className="spin" size={16} /> : <Trash2 size={16} />}
          永久删除本地数据
        </button>
      </footer>
    </Modal>
  );
}
