import { ArrowLeft, RefreshCw, Trash2 } from "lucide-react";
import type { AiLog } from "../../types";

type LogsPageProps = {
  logs: AiLog[];
  busy: string;
  onBack: () => void;
  onClear: () => void;
  onRefresh: () => void;
};

export function LogsPage({ logs, busy, onBack, onClear, onRefresh }: LogsPageProps) {
  return (
    <div className="page-panel">
      <div className="page-heading">
        <h2>AI 调用日志</h2>
        <div className="panel-actions">
          <button onClick={onBack}><ArrowLeft size={16} />返回</button>
          <button onClick={onClear} disabled={busy !== "" || logs.length === 0}>
            <Trash2 size={16} />清空
          </button>
          <button onClick={onRefresh} disabled={busy !== ""}>
            <RefreshCw size={16} />刷新
          </button>
        </div>
      </div>
      <div className="full-log-list">
        {logs.map((log) => (
          <article className={`full-log-item ${log.status}`} key={log.id}>
            <header>
              <div>
                <strong>{log.action}</strong>
                <span>{log.chapter_title || "全局调用"} · {new Date(log.created_at).toLocaleString()}</span>
              </div>
              <span className="log-status">{log.status}</span>
            </header>
            {log.reasoning && <section><h3>思考过程</h3><pre>{log.reasoning}</pre></section>}
            <section><h3>输出文本</h3><pre>{log.content || "无正文内容。"}</pre></section>
            <section><h3>原始响应</h3><pre>{log.raw_response || log.content || "无原始响应。"}</pre></section>
          </article>
        ))}
        {logs.length === 0 && <p className="muted">暂无 AI 调用日志。</p>}
      </div>
    </div>
  );
}
