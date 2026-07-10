import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { DeleteLocalDataDialog } from "./DeleteLocalDataDialog";

describe("DeleteLocalDataDialog", () => {
  it("lists retained data and requires the exact confirmation phrase", () => {
    const onConfirm = vi.fn();
    render(<DeleteLocalDataDialog busy={false} onCancel={vi.fn()} onConfirm={onConfirm} />);

    expect(screen.getByText(/最初导入的原始 TXT 和全部已导出 TXT/)).toBeInTheDocument();
    const confirmButton = screen.getByRole("button", { name: "永久删除本地数据" });
    expect(confirmButton).toBeDisabled();
    const input = screen.getByRole("textbox", { name: "输入删除本地数据确认短语" });
    fireEvent.change(input, { target: { value: "删除本地数据" } });
    expect(confirmButton).toBeDisabled();
    fireEvent.change(input, { target: { value: "删除全部本地数据" } });
    expect(confirmButton).toBeEnabled();
    fireEvent.click(confirmButton);
    expect(onConfirm).toHaveBeenCalledWith("删除全部本地数据");
  });
});
