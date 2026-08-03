import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { NameMappingCleanupDialog } from "./NameMappingCleanupDialog";

describe("NameMappingCleanupDialog", () => {
  it("defaults legacy mappings to deletion and lets the user keep selected entries", () => {
    const onConfirm = vi.fn();
    render(
      <NameMappingCleanupDialog
        busy={false}
        report={{
          managed: [{ source: "萧炎", target: "萧妍" }],
          manual: [],
          legacy_unmanaged: [
            { source: "林动", target: "林筝" },
            { source: "唐三", target: "唐姗" }
          ],
          needs_resolution: true
        }}
        onCancel={vi.fn()}
        onConfirm={onConfirm}
      />
    );

    expect(screen.getByText("林动")).toBeInTheDocument();
    expect(screen.getAllByText("删除旧映射")).toHaveLength(2);
    fireEvent.click(screen.getByRole("checkbox", { name: /林动/ }));
    expect(screen.getByText("保留为手动映射")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "应用处理" }));
    expect(onConfirm).toHaveBeenCalledWith(["林动"]);
  });
});
