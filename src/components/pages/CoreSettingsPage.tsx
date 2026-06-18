import { ArrowLeft, Loader2, Save } from "lucide-react";

type CoreSettingsPageProps = {
  value: string;
  busy: boolean;
  disabled: boolean;
  onChange: (value: string) => void;
  onBack: () => void;
  onSave: () => void;
};

export function CoreSettingsPage({
  value,
  busy,
  disabled,
  onChange,
  onBack,
  onSave
}: CoreSettingsPageProps) {
  return (
    <div className="page-panel">
      <div className="page-heading">
        <h2>核心设定</h2>
        <div className="panel-actions">
          <button onClick={onBack}><ArrowLeft size={16} />返回</button>
          <button onClick={onSave} disabled={busy || disabled}>
            {busy ? <Loader2 className="spin" size={16} /> : <Save size={16} />}保存
          </button>
        </div>
      </div>
      <section className="settings-section core-settings-section">
        <h3>全局改写风格</h3>
        <p className="settings-note">
          核心设定不随小说变化，会在每一次改写和打回重写时发送给 AI，并作为最高优先级的写作要求。建议主要填写文风、叙述节奏、描写密度、语气、对白风格、情绪氛围等全局写法；不要写某一本小说的主角姓名、剧情设定、章节内容或临时任务，避免影响其他小说。
        </p>
        <textarea
          className="core-settings-input"
          disabled={disabled}
          value={value}
          onChange={(event) => onChange(event.target.value)}
          placeholder="例如：保持原文轻小说风格，句子自然流畅；减少机械替换感；动作描写细腻但不过度堆砌；对白保留角色原本语气；百合互动要循序渐进，不要突然强行亲密。"
        />
        {!value.trim() && (
          <p className="settings-empty-hint">
            当前未填写核心设定。留空也可以正常改写；如果填写，建议只写长期通用的文风和描写偏好。
          </p>
        )}
      </section>
    </div>
  );
}
