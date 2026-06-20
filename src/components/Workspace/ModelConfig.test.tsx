import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { emptyProfile } from "../../config/modelRecommendations";
import { ModelConfig } from "./ModelConfig";

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
    expect(screen.getByRole("radio", { name: "关闭" })).toBeDisabled();
    expect(screen.getByRole("radio", { name: "开启" })).toBeDisabled();
  });
});
