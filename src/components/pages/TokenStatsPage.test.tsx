import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { TokenStatsPage } from "./TokenStatsPage";

describe("TokenStatsPage", () => {
  it("renders totals by model and refreshes the selected date range", () => {
    const onRefresh = vi.fn();
    render(
      <TokenStatsPage
        report={{
          start_date: "2026-06-01",
          end_date: "2026-06-30",
          requests: 3,
          input_tokens: 1200,
          output_tokens: 300,
          models: [{
            profile_id: "p1",
            profile_name: "主力模型",
            model: "deepseek-v4-pro",
            requests: 3,
            input_tokens: 1200,
            output_tokens: 300,
            days: [
              { date: "2026-06-03", requests: 2, input_tokens: 800, output_tokens: 200 },
              { date: "2026-06-04", requests: 1, input_tokens: 400, output_tokens: 100 }
            ]
          }]
        }}
        startDate="2026-06-01"
        endDate="2026-06-30"
        busy={false}
        onStartDateChange={vi.fn()}
        onEndDateChange={vi.fn()}
        onRefresh={onRefresh}
        onBack={vi.fn()}
      />
    );

    expect(screen.getByText("deepseek-v4-pro")).toBeInTheDocument();
    expect(screen.getAllByText("1,200")).toHaveLength(2);
    expect(screen.getByRole("img", { name: "每日 API 请求次数" })).toBeInTheDocument();
    expect(screen.getByRole("img", { name: "每日输入和输出 Token" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "统计" }));
    expect(onRefresh).toHaveBeenCalledOnce();
  });
});
