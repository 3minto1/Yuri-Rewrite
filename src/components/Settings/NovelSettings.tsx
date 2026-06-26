import { ArrowLeft, Loader2, Plus, Save, Trash2 } from "lucide-react";
import { useEffect, useState, type Dispatch, type SetStateAction } from "react";
import type { NovelSettingsDraft } from "../../types";
import {
  parseAdditionalNameMappings,
  serializeAdditionalNameMappings,
  type AdditionalNameMappingRow
} from "./additionalNameMappings";
import {
  parseRelationshipTargets,
  serializeRelationshipTargets,
  type RelationshipTargetRow
} from "./relationshipTargets";

type NovelSettingsFieldsProps = {
  draft: NovelSettingsDraft;
  setDraft: Dispatch<SetStateAction<NovelSettingsDraft>>;
  disabled: boolean;
};

function nextAdditionalRowId() {
  return `additional-name-${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

function nextAliasRowId() {
  return `protagonist-alias-${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

function nextRelationshipRowId() {
  return `relationship-target-${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

function useProtagonistAliasRows(draft: NovelSettingsDraft, setDraft: Dispatch<SetStateAction<NovelSettingsDraft>>) {
  const [rows, setRows] = useState<AdditionalNameMappingRow[]>(() =>
    parseAdditionalNameMappings(draft.protagonist_aliases)
  );
  const [syncedValue, setSyncedValue] = useState(draft.protagonist_aliases);

  useEffect(() => {
    if (draft.protagonist_aliases === syncedValue) return;
    setRows(parseAdditionalNameMappings(draft.protagonist_aliases));
    setSyncedValue(draft.protagonist_aliases);
  }, [draft.protagonist_aliases, syncedValue]);

  function commit(nextRows: AdditionalNameMappingRow[]) {
    setRows(nextRows);
    const protagonist_aliases = serializeAdditionalNameMappings(nextRows);
    setSyncedValue(protagonist_aliases);
    setDraft((current) => ({ ...current, protagonist_aliases }));
  }

  return { rows, commit };
}

function useAdditionalNameRows(draft: NovelSettingsDraft, setDraft: Dispatch<SetStateAction<NovelSettingsDraft>>) {
  const [rows, setRows] = useState<AdditionalNameMappingRow[]>(() =>
    parseAdditionalNameMappings(draft.additional_feminize_names)
  );
  const [syncedValue, setSyncedValue] = useState(draft.additional_feminize_names);

  useEffect(() => {
    if (draft.additional_feminize_names === syncedValue) return;
    setRows(parseAdditionalNameMappings(draft.additional_feminize_names));
    setSyncedValue(draft.additional_feminize_names);
  }, [draft.additional_feminize_names, syncedValue]);

  function commit(nextRows: AdditionalNameMappingRow[]) {
    setRows(nextRows);
    const additional_feminize_names = serializeAdditionalNameMappings(nextRows);
    setSyncedValue(additional_feminize_names);
    setDraft((current) => ({ ...current, additional_feminize_names }));
  }

  return { rows, commit };
}

function useRelationshipTargetRows(draft: NovelSettingsDraft, setDraft: Dispatch<SetStateAction<NovelSettingsDraft>>) {
  const [rows, setRows] = useState<RelationshipTargetRow[]>(() =>
    parseRelationshipTargets(draft.relationship_targets)
  );
  const [syncedValue, setSyncedValue] = useState(draft.relationship_targets);

  useEffect(() => {
    if (draft.relationship_targets === syncedValue) return;
    setRows(parseRelationshipTargets(draft.relationship_targets));
    setSyncedValue(draft.relationship_targets);
  }, [draft.relationship_targets, syncedValue]);

  function commit(nextRows: RelationshipTargetRow[]) {
    setRows(nextRows);
    const relationship_targets = serializeRelationshipTargets(nextRows);
    setSyncedValue(relationship_targets);
    setDraft((current) => ({ ...current, relationship_targets }));
  }

  return { rows, commit };
}

function modeLabel(mode: "strict" | "creative") {
  return mode === "strict" ? "严谨模式" : "创意模式";
}

function previewLines(value: string) {
  return value.trim() ? value.trim().split(/\r?\n/u) : [];
}

function truncatePreview(value: string) {
  return value.length > 220 ? `${value.slice(0, 220)}...` : value;
}

export function NovelSettingsFields({ draft, setDraft, disabled }: NovelSettingsFieldsProps) {
  const { rows: protagonistAliasRows, commit: commitProtagonistAliasRows } = useProtagonistAliasRows(draft, setDraft);
  const { rows: additionalNameRows, commit: commitAdditionalNameRows } = useAdditionalNameRows(draft, setDraft);
  const { rows: relationshipRows, commit: commitRelationshipRows } = useRelationshipTargetRows(draft, setDraft);
  const [activeTab, setActiveTab] = useState<"basic" | "advanced" | "preview">("basic");
  const protagonistAliasPreviewLines = previewLines(serializeAdditionalNameMappings(parseAdditionalNameMappings(draft.protagonist_aliases)));

  function updateProtagonistAliasRow(index: number, patch: Partial<AdditionalNameMappingRow>) {
    commitProtagonistAliasRows(protagonistAliasRows.map((row, rowIndex) => rowIndex === index ? { ...row, ...patch } : row));
  }

  function addProtagonistAliasRow() {
    commitProtagonistAliasRows([...protagonistAliasRows, { id: nextAliasRowId(), source: "", target: "" }]);
  }

  function removeProtagonistAliasRow(index: number) {
    const nextRows = protagonistAliasRows.filter((_, rowIndex) => rowIndex !== index);
    commitProtagonistAliasRows(nextRows.length ? nextRows : [{ id: nextAliasRowId(), source: "", target: "" }]);
  }

  function updateAdditionalNameRow(index: number, patch: Partial<AdditionalNameMappingRow>) {
    commitAdditionalNameRows(additionalNameRows.map((row, rowIndex) => rowIndex === index ? { ...row, ...patch } : row));
  }

  function addAdditionalNameRow() {
    commitAdditionalNameRows([...additionalNameRows, { id: nextAdditionalRowId(), source: "", target: "" }]);
  }

  function removeAdditionalNameRow(index: number) {
    const nextRows = additionalNameRows.filter((_, rowIndex) => rowIndex !== index);
    commitAdditionalNameRows(nextRows.length ? nextRows : [{ id: nextAdditionalRowId(), source: "", target: "" }]);
  }

  function updateRelationshipRow(index: number, patch: Partial<RelationshipTargetRow>) {
    commitRelationshipRows(relationshipRows.map((row, rowIndex) => rowIndex === index ? { ...row, ...patch } : row));
  }

  function addRelationshipRow() {
    commitRelationshipRows([...relationshipRows, { id: nextRelationshipRowId(), name: "", relationship: "", notes: "" }]);
  }

  function removeRelationshipRow(index: number) {
    const nextRows = relationshipRows.filter((_, rowIndex) => rowIndex !== index);
    commitRelationshipRows(nextRows.length ? nextRows : [{ id: nextRelationshipRowId(), name: "", relationship: "", notes: "" }]);
  }

  const storedRelationshipRows = parseRelationshipTargets(draft.relationship_targets).filter((row) => row.name.trim());
  const advancedPreview = draft.advanced_settings.trim();

  return (
    <div className="novel-settings-tabs">
      <div className="settings-tabs" role="tablist" aria-label="基本设定分页">
        <button type="button" role="tab" aria-selected={activeTab === "basic"} className={activeTab === "basic" ? "active" : ""} onClick={() => setActiveTab("basic")}>基础设定</button>
        <button type="button" role="tab" aria-selected={activeTab === "advanced"} className={activeTab === "advanced" ? "active" : ""} onClick={() => setActiveTab("advanced")}>高级设定</button>
        <button type="button" role="tab" aria-selected={activeTab === "preview"} className={activeTab === "preview" ? "active" : ""} onClick={() => setActiveTab("preview")}>设定预览</button>
      </div>

      {activeTab === "basic" && (
        <fieldset className="novel-settings-form" disabled={disabled}>
          <section className="settings-section novel-settings-section">
            <div className="settings-section-heading">
              <h3>主角设定</h3>
            </div>
            <div className="form-grid">
              <label>主角姓名（必填）<input value={draft.protagonist_name} onChange={(event) => setDraft({ ...draft, protagonist_name: event.target.value })} /></label>
              <label className="settings-rewritten-name-field">改写后姓名（选填）<input value={draft.rewritten_protagonist_name} onChange={(event) => setDraft({ ...draft, rewritten_protagonist_name: event.target.value })} placeholder="留空则让 AI 生成改写后姓名" /></label>
            </div>
            <div className="settings-subsection-heading">
              <h4>主角别名</h4>
              <span>同一主角在原文中的其他称呼，可指定改写后别名。</span>
            </div>
            <div className="additional-name-list protagonist-alias-list">
              {protagonistAliasRows.map((row, index) => (
                <div className="additional-name-row" key={row.id}>
                  <label>
                    原别名
                    <input
                      value={row.source}
                      onChange={(event) => updateProtagonistAliasRow(index, { source: event.target.value })}
                    />
                  </label>
                  <label>
                    改写后别名（可选）
                    <input
                      value={row.target}
                      onChange={(event) => updateProtagonistAliasRow(index, { target: event.target.value })}
                      placeholder="留空则让 AI 生成"
                    />
                  </label>
                  <button
                    type="button"
                    className="icon-button additional-name-remove"
                    aria-label={`删除主角别名 ${index + 1}`}
                    title="删除"
                    onClick={() => removeProtagonistAliasRow(index)}
                  >
                    <Trash2 size={17} />
                  </button>
                </div>
              ))}
            </div>
            <button type="button" className="additional-name-add" onClick={addProtagonistAliasRow}>
              <Plus size={17} />添加
            </button>
            <p className="settings-note">只填原别名时会由 AI 生成女性化别名；填写改写后别名时会强制使用该称呼。</p>
          </section>

          <section className="settings-section novel-settings-section">
            <div className="settings-section-heading">
              <h3>其他女性化姓名</h3>
            </div>
            <div className="additional-name-list">
              {additionalNameRows.map((row, index) => (
                <div className="additional-name-row" key={row.id}>
                  <label>
                    原姓名
                    <input
                      value={row.source}
                      onChange={(event) => updateAdditionalNameRow(index, { source: event.target.value })}
                    />
                  </label>
                  <label>
                    改写后姓名（可选）
                    <input
                      value={row.target}
                      onChange={(event) => updateAdditionalNameRow(index, { target: event.target.value })}
                      placeholder="留空则让 AI 生成"
                    />
                  </label>
                  <button
                    type="button"
                    className="icon-button additional-name-remove"
                    aria-label={`删除其他女性化姓名 ${index + 1}`}
                    title="删除"
                    onClick={() => removeAdditionalNameRow(index)}
                  >
                    <Trash2 size={17} />
                  </button>
                </div>
              ))}
            </div>
            <button type="button" className="additional-name-add" onClick={addAdditionalNameRow}>
              <Plus size={17} />添加
            </button>
            <p className="settings-note">只填原姓名时会由 AI 生成女性化姓名；填写改写后姓名时会强制使用该姓名。</p>
          </section>

          <section className="settings-section novel-settings-section">
            <div className="settings-section-heading">
              <h3>女主候选 / 关系对象</h3>
            </div>
            <div className="relationship-target-list">
              {relationshipRows.map((row, index) => (
                <div className="relationship-target-row" key={row.id}>
                  <label>
                    姓名
                    <input value={row.name} onChange={(event) => updateRelationshipRow(index, { name: event.target.value })} />
                  </label>
                  <label>
                    关系定位
                    <input value={row.relationship} onChange={(event) => updateRelationshipRow(index, { relationship: event.target.value })} />
                  </label>
                  <label>
                    互动倾向/备注
                    <input value={row.notes} onChange={(event) => updateRelationshipRow(index, { notes: event.target.value })} />
                  </label>
                  <button
                    type="button"
                    className="icon-button additional-name-remove"
                    aria-label={`删除女主候选 ${index + 1}`}
                    title="删除"
                    onClick={() => removeRelationshipRow(index)}
                  >
                    <Trash2 size={17} />
                  </button>
                </div>
              ))}
            </div>
            <button type="button" className="additional-name-add" onClick={addRelationshipRow}>
              <Plus size={17} />添加
            </button>
            <p className="settings-note">用于提示模型强化重点百合互动和关系连续性，不会把该角色自动加入强制改名名单。</p>
          </section>

          <section className="settings-section novel-settings-section">
            <div className="settings-section-heading">
              <h3>外观与模式</h3>
            </div>
            <div className="form-grid">
              <label>身材<select value={draft.bust} onChange={(event) => setDraft({ ...draft, bust: event.target.value })}><option value="平胸">平胸</option><option value="巨乳">巨乳</option></select></label>
              <label>体型<select value={draft.body_type} onChange={(event) => setDraft({ ...draft, body_type: event.target.value })}><option value="萝莉">萝莉</option><option value="御姐">御姐</option><option value="少女">少女</option></select></label>
              <div className="mode-field settings-mode-field">
                <span>改写模式</span>
                <div className="mode-toggle" role="radiogroup" aria-label="改写模式">
                  {(["strict", "creative"] as const).map((mode) => (
                    <button
                      type="button"
                      key={mode}
                      className={draft.rewrite_mode === mode ? "active" : ""}
                      title={mode === "strict" ? "AI 会更加忠于原文，不做过大改动" : "AI 会更加有创意，可能产生较大改动"}
                      aria-checked={draft.rewrite_mode === mode}
                      role="radio"
                      onClick={() => setDraft({ ...draft, rewrite_mode: mode })}
                    >{modeLabel(mode)}</button>
                  ))}
                </div>
              </div>
            </div>
          </section>
        </fieldset>
      )}

      {activeTab === "advanced" && (
        <fieldset className="novel-settings-form" disabled={disabled}>
          <section className="settings-section novel-settings-section">
            <div className="settings-section-heading">
              <h3>高级设定</h3>
            </div>
            <label className="advanced-settings-field">
              <textarea
                aria-label="自定义设定"
                className="advanced-settings-input"
                value={draft.advanced_settings}
                onChange={(event) => setDraft({ ...draft, advanced_settings: event.target.value })}
                placeholder="你可以自由输入需要加入的设定、风格或限制"
              />
            </label>
          </section>
        </fieldset>
      )}

      {activeTab === "preview" && (
        <section className="settings-section novel-settings-section settings-preview-section" aria-label="设定预览">
          <div className="settings-section-heading">
            <h3>设定预览</h3>
            <span>本地摘要，不代表完整最终 prompt。</span>
          </div>
          <dl className="settings-preview-list">
            <div><dt>主角</dt><dd>{draft.protagonist_name.trim() || "未填写"} -&gt; {draft.rewritten_protagonist_name.trim() || "由 AI 自动生成"}</dd></div>
            <div><dt>主角别名</dt><dd>{protagonistAliasPreviewLines.length ? protagonistAliasPreviewLines.map((line) => <span key={line}>{line.includes(" -> ") ? line : `${line} -> 由 AI 自动生成`}</span>) : "无"}</dd></div>
            <div><dt>其他女性化姓名</dt><dd>{previewLines(draft.additional_feminize_names).length ? previewLines(draft.additional_feminize_names).map((line) => <span key={line}>{line}</span>) : "无"}</dd></div>
            <div><dt>女主候选</dt><dd>{storedRelationshipRows.length ? storedRelationshipRows.map((row) => <span key={`${row.name}-${row.relationship}-${row.notes}`}>{row.name}{row.relationship ? `（${row.relationship}）` : ""}{row.notes ? `：${row.notes}` : ""}</span>) : "无"}</dd></div>
            <div><dt>外观与模式</dt><dd>{draft.bust} / {draft.body_type} / {modeLabel(draft.rewrite_mode)}</dd></div>
            <div><dt>高级设定</dt><dd>{advancedPreview ? truncatePreview(advancedPreview) : "未填写"}</dd></div>
          </dl>
        </section>
      )}
    </div>
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
    <div className="page-panel novel-settings-page">
      <div className="page-heading">
        <div>
          <h2>设定</h2>
          <p>{hasNovel ? "配置主角改写、额外姓名映射、外观模式和高级设定。" : "请先导入小说。"}</p>
        </div>
        <div className="panel-actions">
          <button onClick={onBack}><ArrowLeft size={16} />返回</button>
          <button className="action-primary" onClick={onSave} disabled={!hasNovel || busy === "novel-settings" || disabled}>
            {busy === "novel-settings" ? <Loader2 className="spin" size={16} /> : <Save size={16} />}保存
          </button>
        </div>
      </div>
      {hasNovel ? (
        <NovelSettingsFields draft={draft} setDraft={setDraft} disabled={disabled} />
      ) : <p className="muted">请先导入小说。</p>}
    </div>
  );
}
