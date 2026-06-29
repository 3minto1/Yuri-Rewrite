import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { AiLog, AiLogDaySummary } from "../../types";
import { LogsPage } from "./LogsPage";

const days: AiLogDaySummary[] = [
  { date: "2026-06-28", count: 2 },
  { date: "2026-06-27", count: 0 },
  { date: "2026-06-26", count: 1 }
];

const logs: AiLog[] = [
  {
    id: "log-1",
    novel_id: "novel-1",
    profile_id: "profile-1",
    action: "批次改写",
    chapter_title: "第一章",
    status: "success",
    content: "第一条日志",
    raw_response: "原始响应",
    created_at: "2026-06-28T12:00:00+08:00"
  }
];

function renderPage(overrides: Partial<Parameters<typeof LogsPage>[0]> = {}) {
  const props = {
    logs,
    days,
    selectedDate: "2026-06-28",
    busy: "",
    onBack: vi.fn(),
    onClear: vi.fn(),
    onRefresh: vi.fn(),
    onSelectDate: vi.fn(),
    ...overrides
  };
  render(<LogsPage {...props} />);
  return props;
}

describe("LogsPage", () => {
  afterEach(cleanup);

  it("shows recent log days and the selected day's logs", () => {
    renderPage();

    const dateTabs = screen.getByLabelText("日志日期");
    expect(within(dateTabs).getByRole("button", { name: /2026-06-28.*2/ })).toHaveClass("active");
    expect(within(dateTabs).getByRole("button", { name: /2026-06-27.*0/ })).toBeInTheDocument();
    expect(screen.getByText("第一条日志")).toBeInTheDocument();
  });

  it("keeps large log details collapsed until requested", () => {
    renderPage();

    expect(screen.queryByText("原始响应")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "展开详情" }));

    expect(screen.getAllByText("原始响应").length).toBeGreaterThan(0);
    expect(screen.getByText("输出文本")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "收起详情" })).toHaveAttribute("aria-expanded", "true");
  });

  it("only renders visible log cards while scrolling large days", () => {
    const manyLogs = Array.from({ length: 80 }, (_, index): AiLog => ({
      id: `log-${index + 1}`,
      novel_id: "novel-1",
      profile_id: "profile-1",
      action: `日志 ${index + 1}`,
      chapter_title: `第 ${index + 1} 章`,
      status: "success",
      content: `正文 ${index + 1}`,
      raw_response: `原始响应 ${index + 1}`,
      created_at: `2026-06-28T12:${String(index % 60).padStart(2, "0")}:00+08:00`
    }));
    renderPage({
      logs: manyLogs,
      days: [{ date: "2026-06-28", count: manyLogs.length }]
    });

    expect(screen.getByText("日志 1")).toBeInTheDocument();
    expect(screen.queryByText("日志 50")).not.toBeInTheDocument();

    fireEvent.scroll(screen.getByLabelText("日志内容"), { target: { scrollTop: 6000 } });

    expect(screen.getByText("日志 50")).toBeInTheDocument();
  });

  it("requests a date when its day tab is clicked", () => {
    const props = renderPage();

    fireEvent.click(screen.getByRole("button", { name: /2026-06-26.*1/ }));

    expect(props.onSelectDate).toHaveBeenCalledWith("2026-06-26");
  });

  it("distinguishes recent-empty and selected-day-empty states", () => {
    const { rerender } = render(<LogsPage
      logs={[]}
      days={days}
      selectedDate="2026-06-27"
      busy=""
      onBack={vi.fn()}
      onClear={vi.fn()}
      onRefresh={vi.fn()}
      onSelectDate={vi.fn()}
    />);

    expect(screen.getByText("2026-06-27 暂无 AI 调用日志。")).toBeInTheDocument();

    rerender(<LogsPage
      logs={[]}
      days={days.map((day) => ({ ...day, count: 0 }))}
      selectedDate="2026-06-28"
      busy=""
      onBack={vi.fn()}
      onClear={vi.fn()}
      onRefresh={vi.fn()}
      onSelectDate={vi.fn()}
    />);

    expect(screen.getByText("最近 7 天暂无 AI 调用日志。")).toBeInTheDocument();
  });
});
