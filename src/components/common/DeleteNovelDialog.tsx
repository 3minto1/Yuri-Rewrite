import { Loader2, Trash2, X } from "lucide-react";
import type { Novel } from "../../types";
import { Modal } from "./Modal";

type DeleteNovelDialogProps = {
  busy: boolean;
  novel: Novel;
  onCancel: () => void;
  onConfirm: () => void;
};

export function DeleteNovelDialog({ busy, novel, onCancel, onConfirm }: DeleteNovelDialogProps) {
  return (
    <Modal className="settings-dialog delete-novel-dialog" labelledBy="delete-novel-dialog-title">
      <header className="dialog-titlebar">
        <h2 id="delete-novel-dialog-title">确认删除小说</h2>
        <button
          className="dialog-close"
          type="button"
          aria-label="关闭删除小说确认框"
          title="关闭"
          onClick={onCancel}
          disabled={busy}
        >
          <X size={16} />
        </button>
      </header>
      <div className="dialog-body delete-novel-dialog-body">
        <p>
          确定永久删除 <strong>《{novel.title}》</strong>？此操作无法撤销。
        </p>
        <div className="delete-scope delete-scope-danger">
          <strong>以下本地数据和文件会一起删除：</strong>
          <ul>
            <li>小说记录、导入后的章节正文和章节状态</li>
            <li>章节批次记录及软件内部生成的批次 TXT 文件</li>
            <li>小说基本设定、分析结果和一致性资产</li>
            <li>改写稿、任务记录及该小说相关的 AI 日志</li>
            <li>该小说生成的审查警告日志</li>
          </ul>
        </div>
        <div className="delete-scope delete-scope-retained">
          <strong>以下文件不会删除：</strong>
          <ul>
            <li>最初导入的原始 TXT 文件</li>
            <li>已经导出到输出目录的改写 TXT 文件</li>
          </ul>
        </div>
      </div>
      <footer className="dialog-actions">
        <button type="button" onClick={onCancel} disabled={busy}>
          取消
        </button>
        <button className="dialog-danger" type="button" onClick={onConfirm} disabled={busy}>
          {busy ? <Loader2 className="spin" size={16} /> : <Trash2 size={16} />}
          确认删除
        </button>
      </footer>
    </Modal>
  );
}
