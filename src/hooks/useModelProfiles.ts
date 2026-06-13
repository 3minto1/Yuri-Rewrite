import { useMemo, useState } from "react";
import type { ModelProfile, ProfileDraft } from "../types";

export function useModelProfiles(initialDraft: ProfileDraft) {
  const [profiles, setProfiles] = useState<ModelProfile[]>([]);
  const [profileDraft, setProfileDraft] = useState(initialDraft);
  const [selectedProfileId, setSelectedProfileId] = useState("");
  const selectedProfile = useMemo(
    () => profiles.find((profile) => profile.id === selectedProfileId),
    [profiles, selectedProfileId]
  );

  return {
    profiles,
    setProfiles,
    profileDraft,
    setProfileDraft,
    selectedProfileId,
    setSelectedProfileId,
    selectedProfile
  };
}
