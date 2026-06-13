import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ErrorBoundary } from "./ErrorBoundary";

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("../../tauriApi", () => ({ invokeCommand: mocks.invoke }));

function BrokenView(): never {
  throw new Error("render failed");
}

describe("ErrorBoundary", () => {
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("shows a fallback and records the render error", async () => {
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    mocks.invoke.mockResolvedValue(undefined);
    render(<ErrorBoundary><BrokenView /></ErrorBoundary>);
    expect(screen.getByRole("alert")).toHaveTextContent("界面出现异常");
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("record_frontend_error", expect.objectContaining({ message: "render failed" })));
  });

  it("falls back to console logging when local logging fails", async () => {
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined);
    mocks.invoke.mockRejectedValue(new Error("disk unavailable"));
    render(<ErrorBoundary><BrokenView /></ErrorBoundary>);
    await waitFor(() => expect(consoleError).toHaveBeenCalledWith("Failed to record frontend error", expect.any(Error), expect.any(String)));
  });
});
