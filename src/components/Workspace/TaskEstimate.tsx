import { Activity, BookOpenText, Clock3, Layers3, Network, Text } from "lucide-react";
import { memo } from "react";
import type { JobEstimate } from "../../types";

type TaskEstimateProps = {
  estimate: JobEstimate;
  formatNumber: (value?: number | null) => string;
  formatSeconds: (value?: number | null) => string;
};

export const TaskEstimate = memo(function TaskEstimate({ estimate, formatNumber, formatSeconds }: TaskEstimateProps) {
  return (
    <section className="estimate-panel" aria-label="任务预估">
      <div className="estimate-heading">
        <h2>任务预估</h2>
        <div className="estimate-heading-actions"><span>并发 {estimate.parallelism} · 复检{estimate.review_enabled ? "开启" : "关闭"}</span></div>
      </div>
      <div className="estimate-grid">
          <div className="estimate-item"><span className="estimate-item-label"><BookOpenText size={14} aria-hidden="true" />全文规模</span><strong>{formatNumber(estimate.novel_chapters)} 章 · {formatNumber(estimate.novel_chars)} 字 · {formatNumber(estimate.novel_batches)} 批</strong></div>
          <div className="estimate-item"><span className="estimate-item-label"><Layers3 size={14} aria-hidden="true" />当前批次</span><strong>{formatNumber(estimate.selected_batch_chapters)} 章 · {formatNumber(estimate.selected_batch_chars)} 字</strong></div>
          <div className="estimate-item"><span className="estimate-item-label"><Network size={14} aria-hidden="true" />预计请求数</span><strong>当前 {formatNumber(estimate.current_batch_requests)} · 全文 {formatNumber(estimate.full_run_requests)}</strong></div>
          <div className="estimate-item"><span className="estimate-item-label"><Clock3 size={14} aria-hidden="true" />预计等待</span><strong>当前 {formatSeconds(estimate.estimated_current_batch_seconds)} · 全文 {formatSeconds(estimate.estimated_full_run_seconds)}</strong></div>
          <div className="estimate-item"><span className="estimate-item-label"><Activity size={14} aria-hidden="true" />历史调用</span><strong>成功 {formatNumber(estimate.recent_success_calls)} · 失败 {formatNumber(estimate.recent_failed_calls)} · 平均 {formatSeconds(estimate.average_call_seconds)}</strong></div>
          <div className="estimate-item"><span className="estimate-item-label"><Text size={14} aria-hidden="true" />历史字符</span><strong>输入 {formatNumber(estimate.average_input_chars)} · 输出 {formatNumber(estimate.average_output_chars)}</strong></div>
      </div>
    </section>
  );
});
