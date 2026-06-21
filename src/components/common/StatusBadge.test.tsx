import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { getStatusTone, StatusBadge } from "./StatusBadge";

describe("StatusBadge", () => {
  it.each([
    ["completed", "success"],
    ["success", "success"],
    ["ok", "success"],
    ["running", "progress"],
    ["processing", "progress"],
    ["paused", "warning"],
    ["warning", "warning"],
    ["failed", "danger"],
    ["error", "danger"],
    ["pending", "neutral"],
    ["terminated", "neutral"],
    ["unknown", "neutral"]
  ] as const)("maps %s to %s", (status, tone) => {
    expect(getStatusTone(status)).toBe(tone);
  });

  it("keeps readable status text alongside the color cue", () => {
    render(<StatusBadge status="completed" label="改写 完成" />);
    expect(screen.getByText("改写 完成").closest(".status-badge")).toHaveClass("status-success");
  });
});
