import { memo } from "react";
import type { ChapterBatch } from "../../types";

type BatchPanelProps = {
  batches: ChapterBatch[];
  selectedBatch?: ChapterBatch;
  selectedBatchId: string;
  onSelect: (batchId: string) => void;
};

function batchOptionLabel(batch: ChapterBatch) {
  const label = batch.label.trim();
  return label.startsWith(`第${batch.batch_index}批`) ? label : `第${batch.batch_index}批：${label}`;
}

export const BatchPanel = memo(function BatchPanel({ batches, selectedBatch, selectedBatchId, onSelect }: BatchPanelProps) {
  return (
    <div className="batch-strip">
      <label>
        当前批次
        <select value={selectedBatchId} onChange={(event) => onSelect(event.target.value)}>
          {batches.map((batch) => (
            <option key={batch.id} value={batch.id}>
              {batchOptionLabel(batch)}
            </option>
          ))}
        </select>
      </label>
      <span>
        {selectedBatch
          ? `将处理第 ${selectedBatch.start_chapter} - ${selectedBatch.end_chapter} 段/章`
          : "暂无批次"}
      </span>
    </div>
  );
});
