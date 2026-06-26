import { useState } from "react";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { NovelSettingsDraft } from "../../types";
import { NovelSettingsView } from "./NovelSettings";

const initialDraft: NovelSettingsDraft = {
  protagonist_name: "东伯雪鹰",
  protagonist_aliases: "雪鹰 -> 雪瑛\n少主",
  rewritten_protagonist_name: "东伯雪瑛",
  additional_feminize_names: "东伯玉 -> 东伯玥\n池丘白",
  bust: "巨乳",
  body_type: "少女",
  rewrite_mode: "creative",
  advanced_settings: "保持原文节奏",
  relationship_targets: JSON.stringify([
    { name: "余靖秋", relationship: "女主候选", notes: "克制暧昧" }
  ])
};

function Harness({
  disabled = false,
  onSave = vi.fn()
}: {
  disabled?: boolean;
  onSave?: (draft: NovelSettingsDraft) => void;
}) {
  const [draft, setDraft] = useState(initialDraft);
  return (
    <NovelSettingsView
      draft={draft}
      setDraft={setDraft}
      disabled={disabled}
      hasNovel
      busy=""
      onBack={vi.fn()}
      onSave={() => onSave(draft)}
    />
  );
}

afterEach(() => cleanup());

describe("NovelSettingsView", () => {
  it("edits additional name mappings and advanced settings on the standalone page", () => {
    const onSave = vi.fn();
    render(<Harness onSave={onSave} />);

    expect(screen.getByRole("heading", { name: "设定" })).toBeInTheDocument();
    const protagonistSection = screen.getByRole("heading", { name: "主角设定" }).closest("section");
    expect(protagonistSection).not.toBeNull();
    expect(within(protagonistSection!).getByLabelText("主角姓名（必填）")).toHaveValue("东伯雪鹰");
    expect(within(protagonistSection!).getByLabelText("改写后姓名（选填）")).toHaveValue("东伯雪瑛");
    expect(within(protagonistSection!).getByDisplayValue("雪鹰")).toBeInTheDocument();
    expect(within(protagonistSection!).getByDisplayValue("雪瑛")).toBeInTheDocument();
    expect(within(protagonistSection!).getByDisplayValue("少主")).toBeInTheDocument();
    expect(screen.getByDisplayValue("东伯玉")).toBeInTheDocument();
    expect(screen.getByDisplayValue("东伯玥")).toBeInTheDocument();
    expect(screen.getByDisplayValue("池丘白")).toBeInTheDocument();
    expect(screen.queryByPlaceholderText(/例如/)).not.toBeInTheDocument();

    fireEvent.click(within(protagonistSection!).getByRole("button", { name: "添加" }));
    let aliasInputs = screen.getAllByLabelText("原别名");
    aliasInputs[aliasInputs.length - 1].focus();
    fireEvent.change(aliasInputs[aliasInputs.length - 1], { target: { value: "小雪" } });
    aliasInputs = screen.getAllByLabelText("原别名");
    expect(aliasInputs[aliasInputs.length - 1]).toHaveFocus();
    const aliasTargetInputs = screen.getAllByLabelText("改写后别名（可选）");
    fireEvent.change(aliasTargetInputs[aliasTargetInputs.length - 1], { target: { value: "小瑛" } });
    fireEvent.click(screen.getByRole("button", { name: "删除主角别名 2" }));

    const additionalSection = screen.getByRole("heading", { name: "其他女性化姓名" }).closest("section");
    expect(additionalSection).not.toBeNull();
    fireEvent.click(within(additionalSection!).getByRole("button", { name: "添加" }));
    let sourceInputs = screen.getAllByLabelText("原姓名");
    sourceInputs[sourceInputs.length - 1].focus();
    fireEvent.change(sourceInputs[sourceInputs.length - 1], { target: { value: "余" } });
    sourceInputs = screen.getAllByLabelText("原姓名");
    expect(sourceInputs[sourceInputs.length - 1]).toHaveFocus();
    fireEvent.change(sourceInputs[sourceInputs.length - 1], { target: { value: "余靖秋" } });
    sourceInputs = screen.getAllByLabelText("原姓名");
    expect(sourceInputs[sourceInputs.length - 1]).toHaveFocus();
    const targetInputs = screen.getAllByLabelText("改写后姓名（可选）");
    fireEvent.change(targetInputs[targetInputs.length - 1], { target: { value: "余静秋" } });

    fireEvent.click(screen.getByRole("button", { name: "删除其他女性化姓名 2" }));

    const relationshipSection = screen.getByRole("heading", { name: "女主候选 / 关系对象" }).closest("section");
    expect(relationshipSection).not.toBeNull();
    expect(within(relationshipSection!).getByDisplayValue("余靖秋")).toBeInTheDocument();
    fireEvent.click(within(relationshipSection!).getByRole("button", { name: "添加" }));
    let relationshipNameInputs = screen.getAllByLabelText("姓名");
    relationshipNameInputs[relationshipNameInputs.length - 1].focus();
    fireEvent.change(relationshipNameInputs[relationshipNameInputs.length - 1], { target: { value: "池" } });
    relationshipNameInputs = screen.getAllByLabelText("姓名");
    expect(relationshipNameInputs[relationshipNameInputs.length - 1]).toHaveFocus();
    fireEvent.change(relationshipNameInputs[relationshipNameInputs.length - 1], { target: { value: "池丘白" } });
    const relationshipInputs = screen.getAllByLabelText("关系定位");
    const notesInputs = screen.getAllByLabelText("互动倾向/备注");
    fireEvent.change(relationshipInputs[relationshipInputs.length - 1], { target: { value: "师姐" } });
    fireEvent.change(notesInputs[notesInputs.length - 1], { target: { value: "慢热信任" } });

    fireEvent.click(screen.getByRole("tab", { name: "高级设定" }));
    fireEvent.change(screen.getByLabelText("自定义设定"), {
      target: { value: "强化对白克制感" }
    });
    fireEvent.click(screen.getByRole("tab", { name: "设定预览" }));
    expect(screen.getByText(/东伯雪鹰/)).toBeInTheDocument();
    expect(screen.getByText(/雪鹰 -> 雪瑛/)).toBeInTheDocument();
    expect(screen.getByText(/小雪 -> 小瑛/)).toBeInTheDocument();
    expect(screen.getByText(/余靖秋（女主候选）：克制暧昧/)).toBeInTheDocument();
    expect(screen.getByText(/池丘白（师姐）：慢热信任/)).toBeInTheDocument();
    expect(screen.getByText(/强化对白克制感/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    expect(onSave).toHaveBeenCalledTimes(1);
    expect(onSave.mock.calls[0][0]).toMatchObject({
      protagonist_aliases: "雪鹰 -> 雪瑛\n小雪 -> 小瑛",
      additional_feminize_names: "东伯玉 -> 东伯玥\n余靖秋 -> 余静秋",
      advanced_settings: "强化对白克制感",
      relationship_targets: JSON.stringify([
        { name: "余靖秋", relationship: "女主候选", notes: "克制暧昧" },
        { name: "池丘白", relationship: "师姐", notes: "慢热信任" }
      ])
    });
  });

  it("disables the form while a task is running", () => {
    render(<Harness disabled />);

    expect(screen.getByRole("button", { name: "保存" })).toBeDisabled();
    expect(screen.getByLabelText("主角姓名（必填）")).toBeDisabled();
    const protagonistSection = screen.getByRole("heading", { name: "主角设定" }).closest("section");
    expect(protagonistSection).not.toBeNull();
    expect(within(protagonistSection!).getByRole("button", { name: "添加" })).toBeDisabled();
    const additionalSection = screen.getByRole("heading", { name: "其他女性化姓名" }).closest("section");
    expect(additionalSection).not.toBeNull();
    expect(within(additionalSection!).getByRole("button", { name: "添加" })).toBeDisabled();
    const relationshipSection = screen.getByRole("heading", { name: "女主候选 / 关系对象" }).closest("section");
    expect(relationshipSection).not.toBeNull();
    expect(within(relationshipSection!).getByRole("button", { name: "添加" })).toBeDisabled();
  });
});
