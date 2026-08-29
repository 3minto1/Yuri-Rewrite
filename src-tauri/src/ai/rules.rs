//! Rewrite rule registry: the single source of truth for behavioral rules
//! that must appear consistently in the rewrite prompt, the review checklist
//! and the revision prompts. Prompt text lives here once; renderers compose
//! the blocks used by the pipeline.

/// Separates the static, per-novel prompt prefix from the dynamic per-shard
/// suffix. Providers that support explicit prompt-cache breakpoints split on
/// this marker; every other provider simply re-joins the two halves, so the
/// marker must never reach a model.
pub(crate) const DYNAMIC_CONTEXT_MARKER: &str = "<<<YURI_REWRITE_DYNAMIC_CONTEXT>>>";

/// Splits a prompt into (static prefix, dynamic suffix). When the marker is
/// absent the whole prompt is treated as static and the suffix is empty.
pub(crate) fn split_dynamic_context(prompt: &str) -> (String, String) {
    match prompt.find(DYNAMIC_CONTEXT_MARKER) {
        Some(index) => {
            let mut prefix = prompt[..index].trim_end().to_string();
            let mut suffix = prompt[index + DYNAMIC_CONTEXT_MARKER.len()..]
                .trim_start()
                .to_string();
            if prefix.is_empty() {
                prefix = prompt.to_string();
                suffix = String::new();
            }
            (prefix, suffix)
        }
        None => (prompt.to_string(), String::new()),
    }
}

/// Re-joins the two halves without the marker for providers whose caching is
/// implicit (OpenAI-compatible prefix caching, Gemini implicit caching).
pub(crate) fn recombine_dynamic_context(prompt: &str) -> String {
    let (prefix, suffix) = split_dynamic_context(prompt);
    if suffix.is_empty() {
        prefix
    } else {
        format!("{}\n\n{}", prefix, suffix)
    }
}

/// Hard rewrite rules rendered into 【改写规则包】. Order matters: it mirrors
/// the priority tiers declared in the rewrite priority block.
pub(crate) fn rewrite_hard_rules() -> Vec<String> {
    vec![
        "标题默认保留原标题和原编号；仅在明确指向主角原名、男性身份/称谓/身体状态时女性化；marker index 不是标题编号。".to_string(),
        "主角及映射表/用户指定角色必须彻底女性化；清除姓名、代词、称谓、身体、外貌气质、动作习惯和旁人评价中的男主残留，让读者看不出主角原本是男性。".to_string(),
        "姓名映射表和用户指定改名最高优先级；已有 `source -> target` 必须全篇统一替换，不得自行改名；未指定目标才按同音/近音生成。".to_string(),
        "所有代词、称谓和性别表达按【人物性别基准表】执行：基准表内的人物严格按标注性别处理，未列入基准表的人物保持原文性别、身份、称谓和代词；性别不明者保持中性或沿用原文；动物、灵兽、妖兽、凶兽、神兽、器灵等非人生物保留原文代词和称谓。".to_string(),
        "群体代词按成员构成判断：含任何未指定男性成员用“他们”或准确群体称呼；只有确认全员女性才用“她们”。".to_string(),
        "保留主线、章节顺序、因果、战力、伏笔和人物动机；外貌、称谓、关系和百合向情绪推进必须承接一致性资产及相邻上下文。".to_string(),
        "仅与主角原名共享单字的未指定 NPC 不得误改；涉及主角旧名、同名、名字来源或以旧名某字为名的句子，必须随主角改名同步消除旧男主姓名矛盾。".to_string(),
    ]
}

/// Conservative cleanup is mostly enforced deterministically: obvious ad or
/// author-note lines are stripped from the model input, and residue is scanned
/// after generation. The prompt only needs a slim fallback rule.
pub(crate) const CLEANUP_RULE: &str =
    "输入正文中不应出现广告或求票求收藏等非正文内容；若仍发现疑似残留，直接删除该行。除此之外只做保守修正（明显错别字、OCR/编码残留、乱码标点），拿不准时必须保留原文。";

fn render_numbered(items: &[String]) -> String {
    items
        .iter()
        .enumerate()
        .map(|(index, item)| format!("{}. {}", index + 1, item))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Renders the numbered hard-rule block for 【改写规则包】.
pub(crate) fn render_rewrite_hard_rules() -> String {
    render_numbered(&rewrite_hard_rules())
}

/// Blocking checklist for the review decision prompt. One line per reviewable
/// rule; keeps review and rewrite prompts aligned on the same rule set.
pub(crate) const REVIEW_BLOCKING_ITEMS: &[&str] = &[
    "主角或指定性转角色在改写稿中仍有明确男性姓名、代词、身份、称谓、身体特征或社会角色残留。",
    "未指定性转的角色被误改性别、亲属关系、称谓或代词；姓名映射表指定的人物必须按映射女性化。",
    "主角与男性角色共同被复数指代，或群体中包含任一未指定性转的男性成员时，改写稿却使用“她们”；此类混合性别群体必须使用“他们”或准确的群体称呼。只有确认全员女性时才使用“她们”。",
    "当前改写稿缺句、重复、串章、空正文、额外章节、marker/章节边界错误，或破坏原文事件顺序、因果、战力、伏笔、人物动机。",
    "外貌、能力状态、关系推进、核心设定或高级设定出现实质矛盾。",
    "主角改名后，改写稿仍保留“同名、原名、旧名、以旧名某字为名、名字含义”等暴露旧主角姓名或与新姓名矛盾的表达。",
    "改写稿仍残留广告、求票求收藏、读者互动等非正文行，或姓名映射表中的 `source -> target` 未被应用。",
    "标题只有在明确出现主角原名，或明确描述主角男性身份、男性称谓、男性身体状态时才算问题；标题编号与 marker index 不一致不是问题。",
];

/// Reviewer calibration exclusions: false-positive patterns that must not be
/// reported as blocking. These mirror the deterministic validators.
pub(crate) const REVIEW_EXCLUSION_ITEMS: &[&str] = &[
    "每个问题必须引用“待审查改写稿”中仍存在的实际文字；只出现在原文中的证据不得列入 issues。",
    "不要把仅与主角原名共享单字的未指定 NPC 当成主角残留。例如主角“石昊”改为“石念昔”时，未被指定或映射的 NPC“秦昊”仍应保留，不是 blocking。",
    "“这家伙”“这个家伙”“家伙”“熊孩子”“孩子”“吃货”“小鬼”等中性昵称本身不是男性残留。只有同处证据明确出现“少年”“男孩”“男子”“公子”“少爷”“小子”“他”等男性指代且确实指向主角，才是 blocking。",
    "原文未明确性别或性别模糊的动物、灵兽、妖兽、凶兽、神兽、器灵等非人生物，改写稿保留原文人称代词和称谓不是问题。",
    "群体成员性别不明时，保留原文“他们”或使用中性群体称呼不是问题；不得仅因群体中包含女性主角就要求改成“她们”。",
    "原文中明显不属于小说正文的作者更新提示、求票求收藏、简短勘误、作者与读者互动、装饰分隔线和孤立乱码允许删除，不得按缺句或内容缺失打回。完本感言、卷末后记、正式后记、番外和实际剧情正文不在此排除项内；无法确定时必须按正文严格审查。",
    "改写稿已应用姓名映射（原文姓名不再出现、映射目标姓名正常出现）不是问题；仅当映射未被应用时才按上一条清单报告。",
];

pub(crate) fn render_review_blocking_checklist() -> String {
    REVIEW_BLOCKING_ITEMS
        .iter()
        .map(|item| format!("- {item}"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn render_review_exclusions() -> String {
    REVIEW_EXCLUSION_ITEMS
        .iter()
        .map(|item| format!("- {item}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dynamic_context_split_round_trips() {
        let prompt = "static rules\n\n<<<YURI_REWRITE_DYNAMIC_CONTEXT>>>\n\ndynamic body";
        let (prefix, suffix) = split_dynamic_context(prompt);
        assert_eq!(prefix, "static rules");
        assert_eq!(suffix, "dynamic body");
        assert_eq!(recombine_dynamic_context(prompt), "static rules\n\ndynamic body");
    }

    #[test]
    fn dynamic_context_split_without_marker_keeps_prompt() {
        let (prefix, suffix) = split_dynamic_context("single block");
        assert_eq!(prefix, "single block");
        assert!(suffix.is_empty());
    }

    #[test]
    fn hard_rules_cover_gender_baseline_and_cleanup_is_slim() {
        let rules = rewrite_hard_rules();
        assert!(rules.iter().any(|rule| rule.contains("人物性别基准表")));
        assert!(CLEANUP_RULE.contains("不应出现广告"));
        assert!(!CLEANUP_RULE.contains("求票求收藏、读者互动、乱码装饰"));
    }

    #[test]
    fn review_renderers_match_rule_set() {
        let checklist = render_review_blocking_checklist();
        assert!(checklist.contains("姓名映射表指定的人物必须按映射女性化"));
        assert!(checklist.contains("姓名映射表"));
        let exclusions = render_review_exclusions();
        assert!(exclusions.contains("非人生物"));
    }
}
