import { ArrowLeft, RefreshCw } from "lucide-react";
import { memo, useMemo } from "react";
import type { TokenUsageDay, TokenUsageReport } from "../../types";

type TokenStatsPageProps = {
  report: TokenUsageReport | null;
  startDate: string;
  endDate: string;
  busy: boolean;
  onStartDateChange: (value: string) => void;
  onEndDateChange: (value: string) => void;
  onRefresh: () => void;
  onBack: () => void;
};

type ChartPoint = TokenUsageDay & { x: number };

function dateRange(startDate: string, endDate: string) {
  const dates: string[] = [];
  const current = new Date(`${startDate}T00:00:00`);
  const end = new Date(`${endDate}T00:00:00`);
  while (!Number.isNaN(current.getTime()) && current <= end && dates.length <= 366) {
    dates.push([
      current.getFullYear(),
      String(current.getMonth() + 1).padStart(2, "0"),
      String(current.getDate()).padStart(2, "0")
    ].join("-"));
    current.setDate(current.getDate() + 1);
  }
  return dates;
}

function compactNumber(value: number) {
  return new Intl.NumberFormat("zh-CN", {
    notation: value >= 10_000 ? "compact" : "standard",
    maximumFractionDigits: 1
  }).format(value);
}

function fullNumber(value: number) {
  return new Intl.NumberFormat("zh-CN").format(value);
}

function buildSeries(days: TokenUsageDay[], startDate: string, endDate: string): ChartPoint[] {
  const byDate = new Map(days.map((day) => [day.date, day]));
  const dates = dateRange(startDate, endDate);
  return dates.map((date, index) => ({
    date,
    requests: byDate.get(date)?.requests ?? 0,
    input_tokens: byDate.get(date)?.input_tokens ?? 0,
    output_tokens: byDate.get(date)?.output_tokens ?? 0,
    x: dates.length <= 1 ? 50 : (index / (dates.length - 1)) * 100
  }));
}

const UsageCharts = memo(function UsageCharts({
  days,
  startDate,
  endDate
}: {
  days: TokenUsageDay[];
  startDate: string;
  endDate: string;
}) {
  const series = useMemo(() => buildSeries(days, startDate, endDate), [days, endDate, startDate]);
  const requestMax = Math.max(1, ...series.map((point) => point.requests));
  const tokenMax = Math.max(1, ...series.map((point) => point.input_tokens + point.output_tokens));
  const requestLine = series
    .map((point) => `${point.x},${92 - (point.requests / requestMax) * 78}`)
    .join(" ");
  const requestArea = `0,92 ${requestLine} 100,92`;
  const barWidth = Math.max(0.8, Math.min(5, 70 / Math.max(series.length, 1)));
  const firstLabel = startDate.slice(5).replace("-", "-");
  const lastLabel = endDate.slice(5).replace("-", "-");

  return (
    <div className="token-chart-grid">
      <section className="token-chart">
        <div className="token-chart-title">请求次数趋势</div>
        <svg viewBox="0 0 100 110" role="img" aria-label="每日 API 请求次数">
          <line x1="0" y1="92" x2="100" y2="92" className="chart-axis" />
          <line x1="0" y1="53" x2="100" y2="53" className="chart-grid-line" />
          <line x1="0" y1="14" x2="100" y2="14" className="chart-grid-line" />
          <polygon points={requestArea} className="request-area" />
          <polyline points={requestLine} className="request-line" />
          <text x="0" y="106">{firstLabel}</text>
          <text x="100" y="106" textAnchor="end">{lastLabel}</text>
          <text x="1" y="11">{compactNumber(requestMax)}</text>
        </svg>
      </section>
      <section className="token-chart">
        <div className="token-chart-title">Token 消耗趋势</div>
        <svg viewBox="0 0 100 110" role="img" aria-label="每日输入和输出 Token">
          <line x1="0" y1="92" x2="100" y2="92" className="chart-axis" />
          <line x1="0" y1="53" x2="100" y2="53" className="chart-grid-line" />
          <line x1="0" y1="14" x2="100" y2="14" className="chart-grid-line" />
          {series.map((point) => {
            const inputHeight = (point.input_tokens / tokenMax) * 78;
            const outputHeight = (point.output_tokens / tokenMax) * 78;
            return (
              <g key={point.date}>
                <rect x={point.x - barWidth / 2} y={92 - inputHeight} width={barWidth} height={inputHeight} className="input-token-bar">
                  <title>{`${point.date} 输入 ${fullNumber(point.input_tokens)}`}</title>
                </rect>
                <rect x={point.x - barWidth / 2} y={92 - inputHeight - outputHeight} width={barWidth} height={outputHeight} className="output-token-bar">
                  <title>{`${point.date} 输出 ${fullNumber(point.output_tokens)}`}</title>
                </rect>
              </g>
            );
          })}
          <text x="0" y="106">{firstLabel}</text>
          <text x="100" y="106" textAnchor="end">{lastLabel}</text>
          <text x="1" y="11">{compactNumber(tokenMax)}</text>
        </svg>
        <div className="token-chart-legend"><span><i className="input-token-dot" />输入</span><span><i className="output-token-dot" />输出</span></div>
      </section>
    </div>
  );
});

export function TokenStatsPage(props: TokenStatsPageProps) {
  const {
    report, startDate, endDate, busy,
    onStartDateChange, onEndDateChange, onRefresh, onBack
  } = props;
  return (
    <div className="page-panel token-stats-page">
      <div className="page-heading">
        <div>
          <h2>Token 统计</h2>
          <p>统计模型接口实际返回的 Token 使用量；未返回 usage 或旧响应已被截断的调用可能无法计入。</p>
        </div>
        <div className="panel-actions">
          <button onClick={onBack}><ArrowLeft size={16} />返回</button>
          <button onClick={onRefresh} disabled={busy || !startDate || !endDate}>
            <RefreshCw className={busy ? "spin" : ""} size={16} />统计
          </button>
        </div>
      </div>
      <div className="token-date-filter">
        <label>开始日期<input type="date" value={startDate} max={endDate} onChange={(event) => onStartDateChange(event.target.value)} /></label>
        <label>结束日期<input type="date" value={endDate} min={startDate} onChange={(event) => onEndDateChange(event.target.value)} /></label>
      </div>
      {report && (
        <div className="token-overview">
          <div><span>总请求次数</span><strong>{fullNumber(report.requests)}</strong></div>
          <div><span>输入 Token</span><strong>{fullNumber(report.input_tokens)}</strong></div>
          <div><span>输出 Token</span><strong>{fullNumber(report.output_tokens)}</strong></div>
          <div><span>总 Token</span><strong>{fullNumber(report.input_tokens + report.output_tokens)}</strong></div>
        </div>
      )}
      <div className="token-model-list">
        {report?.models.map((model) => (
          <article className="token-model-card" key={model.profile_id}>
            <header>
              <div><strong>{model.model}</strong><span>{model.profile_name}</span></div>
              <div className="token-model-totals">
                <span>请求 <b>{fullNumber(model.requests)}</b></span>
                <span>输入 <b>{fullNumber(model.input_tokens)}</b></span>
                <span>输出 <b>{fullNumber(model.output_tokens)}</b></span>
                <span>合计 <b>{fullNumber(model.input_tokens + model.output_tokens)}</b></span>
              </div>
            </header>
            <UsageCharts days={model.days} startDate={report.start_date} endDate={report.end_date} />
          </article>
        ))}
        {!busy && report && report.models.length === 0 && <p className="muted">该日期范围内暂无可统计的模型 Token 记录。</p>}
        {!report && !busy && <p className="muted">选择日期范围后点击“统计”。</p>}
      </div>
    </div>
  );
}
