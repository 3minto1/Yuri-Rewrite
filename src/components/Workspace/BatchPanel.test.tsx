import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { BatchPanel } from "./BatchPanel";
import type { ChapterBatch } from "../../types";

const batches: ChapterBatch[] = [
  {
    id: "batch-1",
    novel_id: "novel-1",
    batch_index: 1,
    label: "1-30章",
    start_chapter: 1,
    end_chapter: 30,
    file_path: "1.txt",
    created_at: "now"
  },
  {
    id: "batch-2",
    novel_id: "novel-1",
    batch_index: 2,
    label: "31-60章",
    start_chapter: 31,
    end_chapter: 60,
    file_path: "2.txt",
    created_at: "now"
  }
];

describe("BatchPanel", () => {
  afterEach(cleanup);

  it("shows the batch number before the chapter range", () => {
    render(<BatchPanel batches={batches} selectedBatch={batches[0]} selectedBatchId="batch-1" onSelect={vi.fn()} />);

    expect(screen.getByRole("option", { name: "第1批：1-30章" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "第2批：31-60章" })).toBeInTheDocument();
  });
});
