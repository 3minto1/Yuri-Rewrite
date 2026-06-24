import { ArrowLeft, FolderOpen, HelpCircle } from "lucide-react";
import type { AppSettings, ModelProfile } from "../../types";

type AppSettingsViewProps = {
  settings: AppSettings;
  profiles: ModelProfile[];
  busy: string;
  processing: boolean;
  allowPausedTaskAdjustments?: boolean;
  onBack: () => void;
  onChooseExportDir: () => void;
  onClearExportDir: () => void;
  onToggleReview: () => void;
  onReviewProfileChange: (profileId: string) => void;
  onAnalysisProfileChange: (profileId: string) => void;
  onBatchSizeChange: (value: 10 | 30 | 50 | 100) => void;
  onParallelismChange: (value: 1 | 3 | 6 | 10 | 25 | 50) => void;
};

export function AppSettingsView(props: AppSettingsViewProps) {
  const { settings, profiles, busy, processing, allowPausedTaskAdjustments = false, onBack, onChooseExportDir, onClearExportDir, onToggleReview, onReviewProfileChange, onAnalysisProfileChange, onBatchSizeChange, onParallelismChange } = props;
  const adjustmentDisabled = processing && !allowPausedTaskAdjustments;
  const batchSize = settings.chapter_batch_size ?? 30;
  const maxParallelism = batchSize === 100 ? 50 : batchSize === 50 ? 25 : 10;
  return (
    <div className="page-panel">
      <div className="page-heading"><h2>设置</h2><div className="panel-actions"><button onClick={onBack}><ArrowLeft size={16} />返回</button></div></div>
      <section className="settings-section">
        <h3>导出目录</h3>
        <div className="setting-row">
          <input readOnly value={settings.export_dir || "默认应用数据目录"} />
          <button onClick={onChooseExportDir} disabled={busy === "choose-export-dir" || processing}><FolderOpen size={16} />选择目录</button>
          <button onClick={onClearExportDir} disabled={!settings.export_dir || busy === "clear-export-dir" || processing}>恢复默认</button>
        </div>
      </section>
      <section className="settings-section">
        <div className="settings-section-heading">
          <h3>分析模型选择</h3>
          <span className="setting-help" tabIndex={0} aria-label="分析模型选择说明"><HelpCircle size={16} /><span className="setting-help-tooltip" role="tooltip">左侧模型下拉框选择的是改写模型。这里可以单独指定分析模型，用于章节分析、原著一致性资产提取和姓名映射候选生成；留空时继续使用当前改写模型分析。</span></span>
        </div>
        <div className="setting-row">
          <select value={settings.analysis_profile_id ?? ""} onChange={(event) => onAnalysisProfileChange(event.target.value)} disabled={busy === "analysis-profile-setting" || adjustmentDisabled} title="选择独立分析模型；留空则使用左侧当前改写模型">
            <option value="">默认使用当前模型分析</option>
            {profiles.map((profile) => <option key={profile.id} value={profile.id}>{profile.model}</option>)}
          </select>
          <span>左侧当前模型仍作为改写模型；一键任务会分别使用这里的分析模型和当前改写模型。</span>
        </div>
      </section>
      <section className="settings-section">
        <div className="settings-section-heading">
          <h3>改写复检</h3>
          <span className="setting-help" tabIndex={0} aria-label="改写复检说明"><HelpCircle size={16} /><span className="setting-help-tooltip" role="tooltip">双专家审查会显著增加请求数和等待时间，但能让改写后的文本逻辑更顺、质量更稳。开启后，每个分片最多可能经历“分析、初稿改写、审查判定、打回重写、审查复判、再次打回重写、第三次审查”七次模型请求。建议为审查专家选择逻辑能力强、JSON 输出稳定、长文本一致性检查更可靠的模型。</span></span>
        </div>
        <div className="setting-toggle-row">
          <button className={settings.review_enabled ? "setting-switch active" : "setting-switch"} onClick={onToggleReview} disabled={busy === "review-setting" || adjustmentDisabled} title="开启复检时AI改写完成后会检查一遍是否有疏漏，会增加改写时间">{settings.review_enabled ? "开启" : "关闭"}</button>
          <span>默认开启，每批改写会由审查专家判定；不通过时打回改写模型重写并复判。优先速度时可关闭。</span>
        </div>
        <div className="setting-row">
          <select value={settings.review_profile_id ?? ""} onChange={(event) => onReviewProfileChange(event.target.value)} disabled={busy === "review-profile-setting" || adjustmentDisabled} title="选择第二个 AI 作为审查专家；留空则使用当前改写模型审查">
            <option value="">使用当前改写模型审查</option>
            {profiles.map((profile) => <option key={profile.id} value={profile.id}>{profile.model}</option>)}
          </select>
          <span>审查专家只判定并列出问题；不通过时会打回改写模型重写，再由审查专家复判。</span>
        </div>
      </section>
      <section className="settings-section">
        <div className="settings-section-heading">
          <h3>每批次章节数</h3>
          <span className="setting-help" tabIndex={0} aria-label="每批次章节数说明"><HelpCircle size={16} /><span className="setting-help-tooltip" role="tooltip">仅影响能够识别正式章节标题的小说。修改后会立即重新生成所有已导入章节型小说的批次文件和批次范围，不会删除章节、分析结果、改写稿、人工编辑或一致性资产。按字符自动分段的小说仍保持每批最多约 10 万字。</span></span>
        </div>
        <div className="setting-toggle-row">
          <div className="mode-toggle mode-toggle-four setting-batch-size" role="radiogroup" aria-label="每批次章节数">
            {([10, 30, 50, 100] as const).map((value) => <button key={value} type="button" className={batchSize === value ? "active" : ""} aria-checked={batchSize === value} role="radio" disabled={busy === "batch-size-setting" || processing} onClick={() => onBatchSizeChange(value)}>{value} 章</button>)}
          </div>
          <span>默认 30 章。任务运行或一键任务暂停时不能重新分批。</span>
        </div>
      </section>
      <section className="settings-section">
        <div className="settings-section-heading">
          <h3>分析/改写并发</h3>
          <span className="setting-help" tabIndex={0} aria-label="分析和改写并发说明"><HelpCircle size={16} /><span className="setting-help-tooltip" role="tooltip">并发表示同一时间最多发起多少个分析或改写请求。数值越高，等待时间通常越短，但同一分钟内消耗的输入、输出和思考 token 也越多，更容易触发服务商的 429 / TPM limit reached 限流。开启改写复检时，审查、打回重写和复判也会占用同一并发队列，实际 token 压力会更高。普通账号建议从 3 或 6 开始；频繁限流、网络失败或分片解析失败时，先降到 1 或 3。</span></span>
        </div>
        <div className="setting-toggle-row">
          <div className="mode-toggle mode-toggle-six setting-parallelism" role="radiogroup" aria-label="分析和改写并发请求数">
            {([1, 3, 6, 10, 25, 50] as const).map((value) => {
              const unavailable = value > maxParallelism;
              return <button key={value} type="button" className={(settings.rewrite_parallelism ?? 10) === value ? "active" : ""} aria-checked={(settings.rewrite_parallelism ?? 10) === value} role="radio" disabled={busy === "parallelism-setting" || adjustmentDisabled || unavailable} title={unavailable ? `每批 ${batchSize} 章时最高可选并发 ${maxParallelism}` : undefined} onClick={() => onParallelismChange(value)}>{value === 1 ? "不并发" : value}</button>;
            })}
          </div>
          <span>默认 10。并发 25 需要每批至少 50 章；并发 50 需要每批 100 章。</span>
        </div>
      </section>
    </div>
  );
}
