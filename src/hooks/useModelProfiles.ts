import { useMemo, useState } from "react";
import { useAppStore } from "../store/appStore";
import type { ProfileDraft } from "../types";

export function useModelProfiles(initialDraft: ProfileDraft) {
  const profiles = useAppStore((state) => state.profiles);
  const setProfiles = useAppStore((state) => state.setProfiles);
  const [profileDraft, setProfileDraft] = useState(initialDraft);
  const selectedProfileId = useAppStore((state) => state.selectedProfileId);
  const setSelectedProfileId = useAppStore((state) => state.setSelectedProfileId);
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
