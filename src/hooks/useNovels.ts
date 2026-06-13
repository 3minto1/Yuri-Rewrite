import { useMemo, useState } from "react";
import type { Novel, NovelDetail, NovelSettingsDraft } from "../types";

export function useNovels(initialSettings: NovelSettingsDraft) {
  const [novels, setNovels] = useState<Novel[]>([]);
  const [detail, setDetail] = useState<NovelDetail | null>(null);
  const [selectedChapterId, setSelectedChapterId] = useState("");
  const [selectedBatchId, setSelectedBatchId] = useState("");
  const [novelSettingsDraft, setNovelSettingsDraft] = useState(initialSettings);

  const selectedChapter = useMemo(
    () => detail?.chapters.find((chapter) => chapter.id === selectedChapterId) ?? detail?.chapters[0],
    [detail, selectedChapterId]
  );
  const selectedBatch = useMemo(
    () => detail?.batches.find((batch) => batch.id === selectedBatchId) ?? detail?.batches[0],
    [detail, selectedBatchId]
  );

  return {
    novels,
    setNovels,
    detail,
    setDetail,
    selectedChapterId,
    setSelectedChapterId,
    selectedBatchId,
    setSelectedBatchId,
    selectedChapter,
    selectedBatch,
    novelSettingsDraft,
    setNovelSettingsDraft
  };
}
