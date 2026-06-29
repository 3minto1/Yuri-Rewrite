import { ArrowLeft, ChevronDown, RefreshCw, Trash2 } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import type { AiLog, AiLogDaySummary } from "../../types";
import { getStatusTone, StatusBadge } from "../common/StatusBadge";

type LogsPageProps = {
  logs: AiLog[];
  days: AiLogDaySummary[];
  selectedDate: string;
  busy: string;
  onBack: () => void;
  onClear: () => void;
  onRefresh: () => void;
  onSelectDate: (date: string) => void;
};

const COLLAPSED_LOG_HEIGHT = 138;
const EXPANDED_LOG_HEIGHT = 560;
const LOG_LIST_OVERSCAN = 6;
const FALLBACK_LOG_LIST_HEIGHT = 720;
const LOG_PREVIEW_LIMIT = 180;

function formatLogDate(date: string) {
  const [year, month, day] = date.split("-").map(Number);
  const value = new Date(year, month - 1, day);
  const weekday = new Intl.DateTimeFormat("zh-CN", { weekday: "short" }).format(value);
  return `${date} ${weekday}`;
}

function logPreview(log: AiLog) {
  const source = log.content || log.reasoning || log.raw_response || "无正文内容。";
  const normalized = source.replace(/\s+/g, " ").trim();
  if (normalized.length <= LOG_PREVIEW_LIMIT) return normalized;
  return `${normalized.slice(0, LOG_PREVIEW_LIMIT)}...`;
}

function lowerBound(values: number[], target: number) {
  let low = 0;
  let high = values.length - 1;
  while (low < high) {
    const mid = Math.floor((low + high) / 2);
    if (values[mid] < target) {
      low = mid + 1;
    } else {
      high = mid;
    }
  }
  return low;
}

export function LogsPage({
  logs,
  days,
  selectedDate,
  busy,
  onBack,
  onClear,
  onRefresh,
  onSelectDate
}: LogsPageProps) {
  const listRef = useRef<HTMLDivElement | null>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [viewportHeight, setViewportHeight] = useState(FALLBACK_LOG_LIST_HEIGHT);
  const [expandedLogIds, setExpandedLogIds] = useState<Set<string>>(() => new Set());
  const totalLogCount = days.length === 0 ? logs.length : days.reduce((sum, day) => sum + day.count, 0);
  const selectedDay = days.find((day) => day.date === selectedDate);
  const visibleLogIds = useMemo(() => new Set(logs.map((log) => log.id)), [logs]);
  const rowHeights = useMemo(
    () => logs.map((log) => (expandedLogIds.has(log.id) ? EXPANDED_LOG_HEIGHT : COLLAPSED_LOG_HEIGHT)),
    [expandedLogIds, logs]
  );
  const rowOffsets = useMemo(() => {
    const offsets = [0];
    for (const height of rowHeights) {
      offsets.push(offsets[offsets.length - 1] + height);
    }
    return offsets;
  }, [rowHeights]);
  const totalListHeight = rowOffsets[rowOffsets.length - 1] ?? 0;
  const visibleRange = useMemo(() => {
    if (logs.length === 0) return { start: 0, end: 0 };
    const start = Math.max(0, lowerBound(rowOffsets, Math.max(0, scrollTop - COLLAPSED_LOG_HEIGHT * LOG_LIST_OVERSCAN)) - 1);
    const end = Math.min(
      logs.length,
      lowerBound(rowOffsets, scrollTop + viewportHeight + COLLAPSED_LOG_HEIGHT * LOG_LIST_OVERSCAN) + 1
    );
    return { start, end: Math.max(end, start + 1) };
  }, [logs.length, rowOffsets, scrollTop, viewportHeight]);
  const virtualLogs = logs.slice(visibleRange.start, visibleRange.end);

  useEffect(() => {
    setScrollTop(0);
    const listNode = listRef.current;
    if (!listNode) return;
    if (typeof listNode.scrollTo === "function") {
      listNode.scrollTo({ top: 0 });
    } else {
      listNode.scrollTop = 0;
    }
  }, [selectedDate]);

  useEffect(() => {
    setExpandedLogIds((previous) => {
      const next = new Set([...previous].filter((id) => visibleLogIds.has(id)));
      return next.size === previous.size ? previous : next;
    });
  }, [visibleLogIds]);

  useEffect(() => {
    const node = listRef.current;
    if (!node) return;
    const updateHeight = () => setViewportHeight(node.clientHeight || FALLBACK_LOG_LIST_HEIGHT);
    updateHeight();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(updateHeight);
    observer.observe(node);
    return () => observer.disconnect();
  }, []);

  function toggleLogDetails(logId: string) {
    setExpandedLogIds((previous) => {
      const next = new Set(previous);
      if (next.has(logId)) {
        next.delete(logId);
      } else {
        next.add(logId);
      }
      return next;
    });
  }

  return (
    <div className="page-panel">
      <div className="page-heading">
        <h2>AI 调用日志</h2>
        <div className="panel-actions">
          <button onClick={onBack}><ArrowLeft size={16} />返回</button>
          <button onClick={onClear} disabled={busy !== "" || totalLogCount === 0}>
            <Trash2 size={16} />清空
          </button>
          <button onClick={onRefresh} disabled={busy !== ""}>
            <RefreshCw size={16} />刷新
          </button>
        </div>
      </div>
      <div className="log-day-tabs" aria-label="日志日期">
        {days.map((day) => (
          <button
            className={day.date === selectedDate ? "active" : ""}
            type="button"
            key={day.date}
            onClick={() => onSelectDate(day.date)}
            disabled={busy !== ""}
          >
            <span>{formatLogDate(day.date)}</span>
            <strong>{day.count}</strong>
          </button>
        ))}
      </div>
      <div
        className="full-log-list"
        aria-label="日志内容"
        ref={listRef}
        onScroll={(event) => setScrollTop(event.currentTarget.scrollTop)}
      >
        {totalLogCount === 0 ? (
          <p className="muted">最近 7 天暂无 AI 调用日志。</p>
        ) : logs.length === 0 ? (
          <p className="muted">{selectedDay ? `${selectedDay.date} 暂无 AI 调用日志。` : "该日期暂无 AI 调用日志。"}</p>
        ) : (
          <div className="full-log-virtual" style={{ height: totalListHeight }}>
            {virtualLogs.map((log, index) => {
              const absoluteIndex = visibleRange.start + index;
              const expanded = expandedLogIds.has(log.id);
              return (
                <article
                  className={`full-log-item status-container status-${getStatusTone(log.status)} ${expanded ? "expanded" : ""}`}
                  key={log.id}
                  style={{
                    height: rowHeights[absoluteIndex],
                    transform: `translateY(${rowOffsets[absoluteIndex]}px)`
                  }}
                >
                  <header>
                    <div>
                      <strong>{log.action}</strong>
                      <span>{log.chapter_title || "全局调用"} · {new Date(log.created_at).toLocaleString()}</span>
                    </div>
                    <div className="log-card-actions">
                      <StatusBadge
                        status={log.status}
                        label={`${log.status}${log.finish_reason ? ` · ${log.finish_reason}` : ""}`}
                      />
                      <button
                        type="button"
                        className="log-detail-toggle"
                        aria-expanded={expanded}
                        onClick={() => toggleLogDetails(log.id)}
                      >
                        <ChevronDown size={15} />
                        {expanded ? "收起详情" : "展开详情"}
                      </button>
                    </div>
                  </header>
                  <p className="log-preview">{logPreview(log)}</p>
                  {expanded && (
                    <div className="log-detail-sections">
                      {log.reasoning && <section><h3>思考过程</h3><pre>{log.reasoning}</pre></section>}
                      <section><h3>输出文本</h3><pre>{log.content || "无正文内容。"}</pre></section>
                      <section><h3>原始响应</h3><pre>{log.raw_response || log.content || "无原始响应。"}</pre></section>
                    </div>
                  )}
                </article>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
