import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { emptyProfile } from "../../config/modelRecommendations";
import { ModelConfig } from "./ModelConfig";

afterEach(cleanup);

describe("ModelConfig", () => {
  it("shows temperature and top p controls with parameter guidance", () => {
    render(
      <ModelConfig
        draft={emptyProfile}
        setDraft={vi.fn()}
        selectedProfile={undefined}
        selectedProfileId=""
        suggestions={[]}
        suggestionsOpen={false}
        busy=""
        processing={false}
        savedApiKeyMask="********"
        onSuggestionsOpenChange={vi.fn()}
        onCreate={vi.fn()}
        onDiagnose={vi.fn()}
        onSave={vi.fn()}
      />
    );

    expect(screen.getByRole("spinbutton", { name: "Temperature" })).toHaveValue(0.7);
    expect(screen.getByRole("spinbutton", { name: "Top P" })).toHaveValue(1);
    expect(screen.getByLabelText("Temperature 参数说明")).toBeInTheDocument();
    expect(screen.getByLabelText("Top P 参数说明")).toBeInTheDocument();
    expect(screen.getByText(/topP 参数控制 AI 响应的多样性/)).toHaveClass(
      "model-parameter-tooltip-right"
    );
    expect(screen.getByText(/修改AI回复的创造力/)).toBeInTheDocument();
    expect(screen.getByLabelText("思考模式说明")).toBeInTheDocument();
    expect(screen.getByText(/非推理型号不支持 reasoning_effort/)).toBeInTheDocument();
    const thinkingGroup = screen.getByRole("radiogroup", { name: "思考模式" });
    expect(within(thinkingGroup).getByRole("radio", { name: "关闭" })).toBeDisabled();
    expect(within(thinkingGroup).getByRole("radio", { name: "开启" })).toBeDisabled();
    expect(screen.getByLabelText("提示词模糊说明")).toBeInTheDocument();
    expect(screen.getByText(/会在发送前进行敏感表达模糊化/)).toHaveClass(
      "prompt-obfuscation-tooltip"
    );
    const obfuscationGroup = screen.getByRole("radiogroup", { name: "提示词模糊" });
    expect(within(obfuscationGroup).getByRole("radio", { name: "关闭" })).toBeChecked();
    expect(within(obfuscationGroup).getByRole("radio", { name: "开启" })).not.toBeChecked();
  });

  it("keeps prompt obfuscation off by default and allows enabling it", () => {
    function Harness() {
      const [draft, setDraft] = useState(emptyProfile);
      return (
        <ModelConfig
          draft={draft}
          setDraft={setDraft}
          selectedProfile={undefined}
          selectedProfileId=""
          suggestions={[]}
          suggestionsOpen={false}
          busy=""
          processing={false}
          savedApiKeyMask="********"
          onSuggestionsOpenChange={() => undefined}
          onCreate={() => undefined}
          onDiagnose={() => undefined}
          onSave={() => undefined}
        />
      );
    }

    render(<Harness />);
    const group = screen.getByRole("radiogroup", { name: "提示词模糊" });
    const off = within(group).getByRole("radio", { name: "关闭" });
    const on = within(group).getByRole("radio", { name: "开启" });
    expect(off).toBeChecked();
    expect(on).not.toBeChecked();

    fireEvent.click(on);

    expect(off).not.toBeChecked();
    expect(on).toBeChecked();
  });
});
