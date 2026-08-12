import { ArrowLeft, Cable, ChevronDown, FilePlus2, HelpCircle, KeyRound, Loader2, Save } from "lucide-react";
import { useEffect, useId, useMemo, useRef, useState, type Dispatch, type KeyboardEvent, type SetStateAction } from "react";
import {
  getProviderBaseUrl,
  getThinkingModeSupport,
  normalizeThinkingMode
} from "../../config/modelRecommendations";
import type { DiscoveredModel, ModelProfile, ProfileDraft } from "../../types";
import { ScrollablePanel } from "../common/ScrollablePanel";

type ModelConfigProps = {
  draft: ProfileDraft;
  setDraft: Dispatch<SetStateAction<ProfileDraft>>;
  selectedProfile?: ModelProfile;
  selectedProfileId: string;
  discoveredModels: DiscoveredModel[] | null;
  suggestionsOpen: boolean;
  busy: string;
  processing: boolean;
  savedApiKeyMask: string;
  onSuggestionsOpenChange: (open: boolean) => void;
  onCreate: () => void;
  onDiscover: () => void;
  onDiagnose: () => void;
  onSave: () => void;
  standalone?: boolean;
  onBack?: () => void;
};

export function ModelConfig(props: ModelConfigProps) {
  const {
    draft, setDraft, selectedProfile, selectedProfileId, discoveredModels, suggestionsOpen,
    busy, processing, savedApiKeyMask, onSuggestionsOpenChange,
    onCreate, onDiscover, onDiagnose, onSave, standalone = false, onBack
  } = props;
  const popupId = useId();
  const triggerRef = useRef<HTMLButtonElement>(null);
  const popupRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const [modelQuery, setModelQuery] = useState("");
  const [activeOptionIndex, setActiveOptionIndex] = useState(0);
  const remoteModelsAvailable = discoveredModels !== null;
  const options = useMemo(() => (discoveredModels ?? []).map((model) => ({
    model: model.id,
    label: model.display_name || model.id,
    owner: model.owner
  })), [discoveredModels]);
  const filteredOptions = useMemo(() => {
    const query = modelQuery.trim().toLocaleLowerCase();
    const matched = query
      ? options.filter((option) => [option.label, option.model, option.owner ?? ""]
        .some((value) => value.toLocaleLowerCase().includes(query)))
      : options;
    return matched.slice(0, 200);
  }, [modelQuery, options]);
  const selectedModelAvailable = discoveredModels === null
    || discoveredModels.some((model) => model.id === draft.model.trim());

  const closeSuggestions = (restoreFocus = false) => {
    onSuggestionsOpenChange(false);
    if (restoreFocus) window.requestAnimationFrame(() => triggerRef.current?.focus());
  };

  const chooseModel = (model: string) => {
    setDraft((current) => normalizeThinkingMode({ ...current, model }));
    closeSuggestions(true);
  };

  useEffect(() => {
    if (!suggestionsOpen) return;
    setModelQuery("");
    setActiveOptionIndex(0);
    window.requestAnimationFrame(() => searchRef.current?.focus());
  }, [suggestionsOpen, remoteModelsAvailable]);

  useEffect(() => {
    setActiveOptionIndex((current) => Math.min(current, Math.max(0, filteredOptions.length - 1)));
  }, [filteredOptions.length]);

  useEffect(() => {
    if (!suggestionsOpen) return;
    document.getElementById(`${popupId}-option-${activeOptionIndex}`)?.scrollIntoView?.({ block: "nearest" });
  }, [activeOptionIndex, popupId, suggestionsOpen]);

  useEffect(() => {
    if (!suggestionsOpen) return;
    const handlePointerDown = (event: MouseEvent) => {
      const target = event.target as Node;
      if (popupRef.current?.contains(target) || triggerRef.current?.contains(target)) return;
      closeSuggestions(false);
    };
    document.addEventListener("mousedown", handlePointerDown);
    return () => document.removeEventListener("mousedown", handlePointerDown);
  }, [suggestionsOpen]);

  const handleSuggestionKeyDown = (event: KeyboardEvent) => {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      closeSuggestions(true);
      return;
    }
    if (event.key === "Tab") {
      closeSuggestions(false);
      return;
    }
    if (!filteredOptions.length) return;
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const delta = event.key === "ArrowDown" ? 1 : -1;
      setActiveOptionIndex((current) => (current + delta + filteredOptions.length) % filteredOptions.length);
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      chooseModel(filteredOptions[activeOptionIndex].model);
    }
  };
  const thinkingSupport = getThinkingModeSupport(draft);
  const updateProviderFields = (updates: Partial<ProfileDraft>) => {
    setDraft((current) => normalizeThinkingMode({ ...current, ...updates }));
  };
  return (
    <section className={standalone ? "panel model-panel model-management-panel" : "panel model-panel"}>
      <div className="panel-heading">
        <div>
          <h2>{standalone ? "模型管理" : "模型配置"}</h2>
          {standalone && <p>配置模型连接、生成参数和本机保存的 API Key。</p>}
        </div>
        <div className="panel-actions">
          <button className="secondary-button" onClick={onCreate} disabled={busy !== "" || processing}><FilePlus2 size={16} />新建</button>
          <button onClick={onDiagnose} disabled={!selectedProfileId || busy === "diagnose" || processing}>
            {busy === "diagnose" ? <Loader2 className="spin" size={16} /> : <KeyRound size={16} />}诊断模型
          </button>
          <button className="action-primary" onClick={onSave} disabled={busy === "profile" || processing}>
            {busy === "profile" ? <Loader2 className="spin" size={16} /> : <Save size={16} />}保存
          </button>
          {standalone && onBack && <button className="secondary-button" onClick={onBack}><ArrowLeft size={16} />返回工作台</button>}
        </div>
      </div>
      <ScrollablePanel className="model-scroll">
        <fieldset className="form-grid model-form-grid" disabled={processing}>
          <legend className="sr-only">模型配置表单</legend>
          <div className="model-form-section-title form-full"><strong>基本信息</strong><span>用于在本机区分不同模型配置</span></div>
          <label>名称<input value={draft.name} onChange={(event) => setDraft({ ...draft, name: event.target.value })} /></label>
          <label>
            Provider
            <select
              value={draft.provider}
              onChange={(event) => {
                const provider = event.target.value;
                updateProviderFields({
                  provider,
                  base_url: getProviderBaseUrl(draft, provider)
                });
              }}
            >
              <option value="openai-compatible">OpenAI 兼容</option>
              <option value="anthropic">Anthropic Messages</option>
              <option value="gemini">Google Gemini</option>
            </select>
          </label>
          <div className="model-form-section-title form-full"><strong>连接与凭据</strong><span>先填写服务地址和 API Key</span></div>
          <label>
            Base URL
            <input value={draft.base_url} onChange={(event) => updateProviderFields({ base_url: event.target.value })} />
            {draft.provider === "anthropic" && (
              <small>填写服务商的 Anthropic Base URL，程序会自动调用 Messages 接口。</small>
            )}
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
          <div className="model-form-section-title form-full"><strong>模型发现</strong><span>连接服务商并从账号可见列表中选择模型</span></div>
          <div className="model-discovery-field form-full">
            <span>模型名</span>
            <div className="model-discovery-row">
              <div className={options.length > 0 ? "model-name-control has-options" : "model-name-control"}>
              <input
                value={draft.model}
                onChange={(event) => updateProviderFields({ model: event.target.value })}
                role="combobox"
                aria-label="模型名"
                aria-autocomplete="list"
                aria-controls={options.length > 0 ? popupId : undefined}
                aria-expanded={suggestionsOpen && options.length > 0}
              />
              {options.length > 0 && (
                <button
                  ref={triggerRef}
                  type="button"
                  className="model-suggestion-trigger"
                  title="选择账号可见模型"
                  aria-label="选择账号可见模型"
                  aria-haspopup="listbox"
                  aria-controls={popupId}
                  aria-expanded={suggestionsOpen}
                  onClick={() => onSuggestionsOpenChange(!suggestionsOpen)}
                ><ChevronDown size={16} /></button>
              )}
              {suggestionsOpen && options.length > 0 && (
                <div ref={popupRef} className="model-suggestion-menu" onKeyDown={handleSuggestionKeyDown}>
                  <div className="model-suggestion-menu-heading">
                    <strong>账号可见模型</strong>
                    <span>{options.length} 项</span>
                  </div>
                  <input
                    ref={searchRef}
                    className="model-suggestion-search"
                    value={modelQuery}
                    onChange={(event) => setModelQuery(event.target.value)}
                    placeholder="搜索模型 ID、名称或所有者"
                    aria-label="搜索账号可见模型"
                    aria-controls={popupId}
                    aria-activedescendant={filteredOptions[activeOptionIndex] ? `${popupId}-option-${activeOptionIndex}` : undefined}
                  />
                  <div id={popupId} className="model-suggestion-list" role="listbox" aria-label="账号可见模型">
                    {filteredOptions.map((option, index) => (
                      <button
                        type="button"
                        id={`${popupId}-option-${index}`}
                        key={option.model}
                        role="option"
                        tabIndex={-1}
                        className={index === activeOptionIndex ? "active" : ""}
                        aria-selected={draft.model === option.model}
                        onMouseEnter={() => setActiveOptionIndex(index)}
                        onClick={() => chooseModel(option.model)}
                      >
                        <span>{option.label}</span>
                        <small>{option.model}{option.owner ? ` · ${option.owner}` : ""}</small>
                      </button>
                    ))}
                    {filteredOptions.length === 0 && <p className="model-suggestion-empty">没有匹配的模型</p>}
                  </div>
                  {options.length > 200 && filteredOptions.length === 200 && (
                    <small className="model-suggestion-limit">当前显示前 200 项，请继续输入关键词缩小范围。</small>
                  )}
                </div>
              )}
              </div>
              <button
                type="button"
                className="model-discover-button"
                onClick={onDiscover}
                disabled={busy !== "" || processing}
              >
                {busy === "discover-models" ? <Loader2 className="spin" size={16} /> : <Cable size={16} />}
                连接并获取模型
              </button>
            </div>
            {remoteModelsAvailable && (
              <small className={selectedModelAvailable ? "model-discovery-note" : "model-discovery-note warning"}>
                {selectedModelAvailable
                  ? `已获取 ${discoveredModels.length} 个账号可见模型；选择后仍需保存，并建议运行“诊断模型”。`
                  : "当前手工模型不在账号可见列表中，已保留原值；你仍可直接保存或重新选择。"}
              </small>
            )}
          </div>
          <div className="model-form-section-title form-full"><strong>生成参数</strong><span>控制随机性、采样和模型思考行为</span></div>
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
          <div className="mode-field thinking-mode-field form-full">
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
          </div>
          <div className="mode-field prompt-obfuscation-field form-full">
            <span className="model-parameter-heading">
              提示词模糊
              <span className="setting-help" tabIndex={0} aria-label="提示词模糊说明">
                <HelpCircle size={15} />
                <span className="setting-help-tooltip prompt-obfuscation-tooltip" role="tooltip">
                  开启后，该模型收到的系统提示词和用户提示词会在发送前进行敏感表达模糊化，以降低内容安全策略拦截概率。
                </span>
              </span>
            </span>
            <div className="mode-toggle" role="radiogroup" aria-label="提示词模糊">
              <button
                type="button"
                className={!draft.prompt_obfuscation_enabled ? "active" : ""}
                role="radio"
                aria-checked={!draft.prompt_obfuscation_enabled}
                onClick={() => setDraft({ ...draft, prompt_obfuscation_enabled: false })}
              >关闭</button>
              <button
                type="button"
                className={draft.prompt_obfuscation_enabled ? "active" : ""}
                role="radio"
                aria-checked={draft.prompt_obfuscation_enabled}
                onClick={() => setDraft({ ...draft, prompt_obfuscation_enabled: true })}
              >开启</button>
            </div>
          </div>
        </fieldset>
      </ScrollablePanel>
    </section>
  );
}
