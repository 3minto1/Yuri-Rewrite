import { ChevronDown, FilePlus2, HelpCircle, KeyRound, Loader2, Save } from "lucide-react";
import type { Dispatch, SetStateAction } from "react";
import {
  getThinkingModeSupport,
  normalizeThinkingMode
} from "../../config/modelRecommendations";
import type { ModelProfile, ProfileDraft } from "../../types";
import { ScrollablePanel } from "../common/ScrollablePanel";

type ModelConfigProps = {
  draft: ProfileDraft;
  setDraft: Dispatch<SetStateAction<ProfileDraft>>;
  selectedProfile?: ModelProfile;
  selectedProfileId: string;
  suggestions: Array<{ label: string; model: string }>;
  suggestionsOpen: boolean;
  busy: string;
  processing: boolean;
  savedApiKeyMask: string;
  onSuggestionsOpenChange: (open: boolean) => void;
  onCreate: () => void;
  onDiagnose: () => void;
  onSave: () => void;
};

export function ModelConfig(props: ModelConfigProps) {
  const {
    draft, setDraft, selectedProfile, selectedProfileId, suggestions, suggestionsOpen,
    busy, processing, savedApiKeyMask, onSuggestionsOpenChange,
    onCreate, onDiagnose, onSave
  } = props;
  const thinkingSupport = getThinkingModeSupport(draft);
  const updateProviderFields = (updates: Partial<ProfileDraft>) => {
    setDraft((current) => normalizeThinkingMode({ ...current, ...updates }));
  };
  return (
    <section className="panel model-panel">
      <div className="panel-heading">
        <h2>模型配置</h2>
        <div className="panel-actions">
          <button onClick={onCreate} disabled={busy !== "" || processing}><FilePlus2 size={16} />新建</button>
          <button onClick={onDiagnose} disabled={!selectedProfileId || busy === "diagnose" || processing}>
            {busy === "diagnose" ? <Loader2 className="spin" size={16} /> : <KeyRound size={16} />}诊断模型
          </button>
          <button onClick={onSave} disabled={busy === "profile" || processing}>
            {busy === "profile" ? <Loader2 className="spin" size={16} /> : <Save size={16} />}保存
          </button>
        </div>
      </div>
      <ScrollablePanel className="model-scroll">
        <fieldset className="form-grid model-form-grid" disabled={processing}>
          <label>名称<input value={draft.name} onChange={(event) => setDraft({ ...draft, name: event.target.value })} /></label>
          <label>
            Provider
            <select
              value={draft.provider}
              onChange={(event) => updateProviderFields({
                provider: event.target.value,
                base_url: event.target.value === "gemini" ? "https://generativelanguage.googleapis.com/v1beta" : draft.base_url
              })}
            >
              <option value="openai-compatible">OpenAI 兼容</option>
              <option value="gemini">Google Gemini</option>
            </select>
          </label>
          <label>Base URL<input value={draft.base_url} onChange={(event) => updateProviderFields({ base_url: event.target.value })} /></label>
          <label>
            模型名
            <div className="model-name-control">
              <input value={draft.model} onChange={(event) => updateProviderFields({ model: event.target.value })} />
              {suggestions.length > 0 && (
                <button
                  type="button"
                  className="model-suggestion-trigger"
                  title="选择检测到的服务商模型"
                  aria-label="选择检测到的服务商模型"
                  aria-expanded={suggestionsOpen}
                  onClick={() => onSuggestionsOpenChange(!suggestionsOpen)}
                ><ChevronDown size={16} /></button>
              )}
              {suggestionsOpen && suggestions.length > 0 && (
                <div className="model-suggestion-menu" role="listbox">
                  {suggestions.map((suggestion) => (
                    <button
                      type="button"
                      key={suggestion.model}
                      role="option"
                      aria-selected={draft.model === suggestion.model}
                      onClick={() => {
                        setDraft((current) => normalizeThinkingMode({
                          ...current,
                          model: suggestion.model
                        }));
                        onSuggestionsOpenChange(false);
                      }}
                    ><span>{suggestion.label}</span><small>{suggestion.model}</small></button>
                  ))}
                </div>
              )}
            </div>
          </label>
          <label>
            <span className="model-parameter-heading">
              Temperature
              <span className="setting-help" tabIndex={0} aria-label="Temperature 参数说明">
                <HelpCircle size={15} />
                <span className="setting-help-tooltip model-parameter-tooltip" role="tooltip">
                  修改AI回复的创造力；值越高，回复变得越随机和有趣，而较低的值则确保更大的稳定性和可靠性。
                </span>
              </span>
            </span>
            <input aria-label="Temperature" type="number" min="0" max="2" step="0.1" value={draft.temperature} onChange={(event) => setDraft({ ...draft, temperature: Number(event.target.value) })} />
          </label>
          <label>
            <span className="model-parameter-heading">
              Top P
              <span className="setting-help" tabIndex={0} aria-label="Top P 参数说明">
                <HelpCircle size={15} />
                <span className="setting-help-tooltip model-parameter-tooltip model-parameter-tooltip-right" role="tooltip">
                  topP 参数控制 AI 响应的多样性：较低的值使输出更集中和可预测，而较高的值则允许更多样化和富有创意的回复。
                </span>
              </span>
            </span>
            <input aria-label="Top P" type="number" min="0" max="1" step="0.05" value={draft.top_p} onChange={(event) => setDraft({ ...draft, top_p: Number(event.target.value) })} />
          </label>
          <label className="mode-field thinking-mode-field form-full">
            <span className="model-parameter-heading">
              思考模式
              <span className="setting-help" tabIndex={0} aria-label="思考模式说明">
                <HelpCircle size={15} />
                <span className="setting-help-tooltip thinking-mode-tooltip" role="tooltip">
                  {thinkingSupport.guidance}
                </span>
              </span>
            </span>
            <div className="mode-toggle mode-toggle-three" role="radiogroup" aria-label="思考模式">
              {(["auto", "off", "on"] as const).map((mode) => (
                <button
                  type="button"
                  key={mode}
                  className={draft.thinking_mode === mode ? "active" : ""}
                  role="radio"
                  aria-checked={draft.thinking_mode === mode}
                  disabled={mode !== "auto" && thinkingSupport.disabledModes.includes(mode)}
                  onClick={() => setDraft({ ...draft, thinking_mode: mode })}
                >{mode === "auto" ? "自动" : mode === "off" ? "关闭" : "开启"}</button>
              ))}
            </div>
          </label>
          <label>
            API Key
            <input
              type="password"
              value={draft.api_key}
              placeholder={selectedProfileId ? "留空则不保存 Key" : "填写 API Key 后保存"}
              onFocus={() => { if (draft.api_key === savedApiKeyMask) setDraft({ ...draft, api_key: "" }); }}
              onChange={(event) => setDraft({ ...draft, api_key: event.target.value })}
            />
            {selectedProfile?.api_key_storage === "database_fallback" && (
              <small className="credential-warning">系统凭据库不可用，API Key 当前以本地数据库兼容模式保存。</small>
            )}
          </label>
        </fieldset>
      </ScrollablePanel>
    </section>
  );
}
