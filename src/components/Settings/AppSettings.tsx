import { ArrowLeft, FolderOpen, HelpCircle } from "lucide-react";
import type { AppSettings, ModelProfile } from "../../types";

type AppSettingsViewProps = {
  settings: AppSettings;
  profiles: ModelProfile[];
  busy: string;
  processing: boolean;
  onBack: () => void;
  onChooseExportDir: () => void;
  onClearExportDir: () => void;
  onToggleReview: () => void;
  onReviewProfileChange: (profileId: string) => void;
  onParallelismChange: (value: 1 | 3 | 6 | 10) => void;
};

export function AppSettingsView(props: AppSettingsViewProps) {
  const { settings, profiles, busy, processing, onBack, onChooseExportDir, onClearExportDir, onToggleReview, onReviewProfileChange, onParallelismChange } = props;
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
          <h3>改写复检</h3>
          <span className="setting-help" tabIndex={0} aria-label="改写复检说明"><HelpCircle size={16} /><span className="setting-help-tooltip" role="tooltip">双专家审查会显著增加请求数和等待时间，但能让改写后的文本逻辑更顺、质量更稳。开启后，每个分片最多可能经历“分析、初稿改写、审查判定、打回重写、审查复判、再次打回重写、第三次审查”七次模型请求。建议为审查专家选择逻辑能力强、JSON 输出稳定、长文本一致性检查更可靠的模型。</span></span>
        </div>
        <div className="setting-toggle-row">
          <button className={settings.review_enabled ? "setting-switch active" : "setting-switch"} onClick={onToggleReview} disabled={busy === "review-setting" || processing} title="开启复检时AI改写完成后会检查一遍是否有疏漏，会增加改写时间">{settings.review_enabled ? "开启" : "关闭"}</button>
          <span>默认关闭，开启后每批改写会由审查专家判定；不通过时打回改写模型重写并复判。</span>
        </div>
        <div className="setting-row">
          <select value={settings.review_profile_id ?? ""} onChange={(event) => onReviewProfileChange(event.target.value)} disabled={busy === "review-profile-setting" || processing} title="选择第二个 AI 作为审查专家；留空则使用当前改写模型审查">
            <option value="">使用当前改写模型审查</option>
            {profiles.map((profile) => <option key={profile.id} value={profile.id}>{profile.model}</option>)}
          </select>
          <span>审查专家只判定并列出问题；不通过时会打回改写模型重写，再由审查专家复判。</span>
        </div>
      </section>
      <section className="settings-section">
        <h3>分析/改写并发</h3>
        <div className="setting-toggle-row" title="较高并发通常可以缩短分析和改写等待时间，但会同时增加请求数量；请求越多，触发限流、网络失败或分片解析失败的概率越高，也可能因每个分片都携带设定和一致性资产而略微增加 token 消耗。若频繁失败或服务商限流，请降低并发数。">
          <div className="mode-toggle mode-toggle-four setting-parallelism" role="radiogroup" aria-label="分析和改写并发请求数">
            {([10, 6, 3, 1] as const).map((value) => <button key={value} type="button" className={(settings.rewrite_parallelism ?? 6) === value ? "active" : ""} aria-checked={(settings.rewrite_parallelism ?? 6) === value} role="radio" disabled={busy === "parallelism-setting" || processing} onClick={() => onParallelismChange(value)}>{value === 1 ? "不并发" : value}</button>)}
          </div>
          <span>默认 6：30 章会拆成 6 个请求，每个约 5 章；分析和改写共用该设置，并尽量共享设定和一致性资产。</span>
        </div>
      </section>
    </div>
  );
}
