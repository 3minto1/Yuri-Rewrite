import { useState } from "react";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Modal } from "./Modal";

describe("Modal", () => {
  afterEach(() => {
    cleanup();
    document.getElementById("root")?.remove();
  });

  it("isolates the background, traps focus, closes with Escape and restores focus", async () => {
    const onClose = vi.fn();
    function Harness() {
      const [open, setOpen] = useState(false);
      return (
        <>
          <button type="button" onClick={() => setOpen(true)}>打开</button>
          {open && (
            <Modal labelledBy="test-dialog-title" onRequestClose={() => { onClose(); setOpen(false); }}>
              <h2 id="test-dialog-title">测试弹窗</h2>
              <button type="button">第一个</button>
              <button type="button">最后一个</button>
            </Modal>
          )}
        </>
      );
    }

    const root = document.createElement("div");
    root.id = "root";
    document.body.appendChild(root);
    render(<Harness />, { container: root });
    const opener = screen.getByRole("button", { name: "打开" });
    opener.focus();
    fireEvent.click(opener);

    await waitFor(() => expect(screen.getByRole("button", { name: "第一个" })).toHaveFocus());
    expect(root).toHaveAttribute("inert");
    expect(root).toHaveAttribute("aria-hidden", "true");

    const last = screen.getByRole("button", { name: "最后一个" });
    last.focus();
    fireEvent.keyDown(document, { key: "Tab" });
    expect(screen.getByRole("button", { name: "第一个" })).toHaveFocus();

    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(onClose).toHaveBeenCalledOnce();
    expect(root).not.toHaveAttribute("inert");
    expect(root).not.toHaveAttribute("aria-hidden");
    expect(opener).toHaveFocus();
  });
});
