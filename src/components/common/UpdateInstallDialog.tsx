import { Download, Loader2, X } from "lucide-react";
import type { UpdateCheckResult } from "../../types";
import { Modal } from "./Modal";

type UpdateInstallDialogProps = {
  busy: boolean;
  processingTaskActive: boolean;
  update: UpdateCheckResult;
  onCancel: () => void;
  onConfirm: () => void;
};

export function UpdateInstallDialog({
  busy,
  processingTaskActive,
  update,
  onCancel,
  onConfirm
}: UpdateInstallDialogProps) {
  const blocked = busy || processingTaskActive;
  return (
    <Modal className="settings-dialog update-install-dialog" labelledBy="update-install-dialog-title">
      <header className="dialog-titlebar">
        <h2 id="update-install-dialog-title">安装 Yuri Rewrite v{update.latest_version}</h2>
        <button
          className="dialog-close"
          type="button"
          aria-label="关闭更新确认框"
          title="关闭"
          onClick={onCancel}
          disabled={busy}
        >
          <X size={16} />
        </button>
      </header>
      <div className="dialog-body update-install-dialog-body">
        <p>更新包下载并校验完成后，软件会短暂关闭，安装完成后自动重新打开。</p>
        <ul>
          <li>本地小说、设置、日志、Token 统计和 API Key 不会被删除。</li>
          <li>GitHub 下载长时间没有进展时，会自动切换内置国内镜像。</li>
          <li>自动安装失败时会恢复旧版本，并在重新打开后显示错误信息。</li>
        </ul>
        {update.auto_install_supported === false && (
          <p className="update-install-warning">
            当前环境只能下载 ZIP 后手动安装：{update.auto_install_reason ?? "当前不是标准 portable 目录。"}
          </p>
        )}
        {processingTaskActive && (
          <p className="update-install-warning">当前有任务正在运行，请等待任务结束后再安装更新。</p>
        )}
      </div>
      <footer className="dialog-actions">
        <button type="button" onClick={onCancel} disabled={busy}>取消</button>
        <button className="dialog-primary" type="button" onClick={onConfirm} disabled={blocked}>
          {busy ? <Loader2 className="spin" size={16} /> : <Download size={16} />}
          {update.auto_install_supported === false ? "下载 ZIP" : "下载并安装"}
        </button>
      </footer>
    </Modal>
  );
}
