import { ArrowLeft, Loader2, Save } from "lucide-react";
import type { Dispatch, SetStateAction } from "react";
import type { NovelSettingsDraft } from "../../types";

type NovelSettingsFieldsProps = {
  draft: NovelSettingsDraft;
  setDraft: Dispatch<SetStateAction<NovelSettingsDraft>>;
  disabled: boolean;
};

export function NovelSettingsFields({ draft, setDraft, disabled }: NovelSettingsFieldsProps) {
  return (
    <fieldset className="form-grid" disabled={disabled}>
      <label>主角姓名（必填）<input value={draft.protagonist_name} onChange={(event) => setDraft({ ...draft, protagonist_name: event.target.value })} placeholder="例如：萧炎" /></label>
      <label className="settings-rewritten-name-field">改写后姓名（选填）<input value={draft.rewritten_protagonist_name} onChange={(event) => setDraft({ ...draft, rewritten_protagonist_name: event.target.value })} placeholder="留空则让AI生成改写后姓名" /></label>
      <label className="settings-additional-names-field">其他需要女性化的人名（选填）<textarea value={draft.additional_feminize_names} onChange={(event) => setDraft({ ...draft, additional_feminize_names: event.target.value })} placeholder="支持逗号或换行分隔" /></label>
      <label>身材<select value={draft.bust} onChange={(event) => setDraft({ ...draft, bust: event.target.value })}><option value="平胸">平胸</option><option value="巨乳">巨乳</option></select></label>
      <label>体型<select value={draft.body_type} onChange={(event) => setDraft({ ...draft, body_type: event.target.value })}><option value="萝莉">萝莉</option><option value="御姐">御姐</option><option value="少女">少女</option></select></label>
      <div className="mode-field">
        <span>改写模式</span>
        <div className="mode-toggle" role="radiogroup" aria-label="改写模式">
          {(["strict", "creative"] as const).map((mode) => (
            <button
              type="button"
              key={mode}
              className={draft.rewrite_mode === mode ? "active" : ""}
              title={mode === "strict" ? "AI会更加忠于原文，不做过大改动" : "AI会更加有创意，可能产生较大改动"}
              aria-checked={draft.rewrite_mode === mode}
              role="radio"
              onClick={() => setDraft({ ...draft, rewrite_mode: mode })}
            >{mode === "strict" ? "严谨模式" : "创意模式"}</button>
          ))}
        </div>
      </div>
    </fieldset>
  );
}

type NovelSettingsViewProps = NovelSettingsFieldsProps & {
  hasNovel: boolean;
  busy: string;
  onBack: () => void;
  onSave: () => void;
};

export function NovelSettingsView({ draft, setDraft, disabled, hasNovel, busy, onBack, onSave }: NovelSettingsViewProps) {
  return (
    <div className="page-panel">
      <div className="page-heading">
        <h2>基本设定</h2>
        <div className="panel-actions">
          <button onClick={onBack}><ArrowLeft size={16} />返回</button>
          <button onClick={onSave} disabled={!hasNovel || busy === "novel-settings" || disabled}>
            {busy === "novel-settings" ? <Loader2 className="spin" size={16} /> : <Save size={16} />}保存
          </button>
        </div>
      </div>
      {hasNovel ? (
        <section className="settings-section novel-settings-section">
          <h3>改写基础规则</h3>
          <NovelSettingsFields draft={draft} setDraft={setDraft} disabled={disabled} />
          <p className="settings-note">分析和改写会自动附带这些设定。主角姓名会按同音或近音原则女性化，例如萧炎改为萧妍，李火旺改为李火婉。</p>
        </section>
      ) : <p className="muted">请先导入小说。</p>}
    </div>
  );
}
