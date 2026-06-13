import { MoreHorizontal, Trash2 } from "lucide-react";
import type { ModelProfile } from "../../types";

type ModelProfilesProps = {
  profiles: ModelProfile[];
  selectedProfileId: string;
  menuOpen: boolean;
  processing: boolean;
  busy: string;
  onSelect: (profileId: string) => void;
  onMenuOpenChange: (open: boolean) => void;
  onDelete: () => void;
};

export function ModelProfiles(props: ModelProfilesProps) {
  const { profiles, selectedProfileId, menuOpen, processing, busy, onSelect, onMenuOpenChange, onDelete } = props;
  return (
    <div className="model-row">
      <select value={selectedProfileId} onChange={(event) => onSelect(event.target.value)} disabled={processing}>
        <option value="">未选择</option>
        {profiles.map((profile) => <option key={profile.id} value={profile.id}>{profile.model}</option>)}
      </select>
      <button className="icon-button menu-trigger" aria-label="打开模型菜单" onClick={() => onMenuOpenChange(!menuOpen)} disabled={!selectedProfileId || processing}><MoreHorizontal size={17} /></button>
      {menuOpen && selectedProfileId && <div className="context-menu"><button onClick={onDelete} disabled={busy === "delete-model" || processing}><Trash2 size={15} />删除当前模型</button></div>}
    </div>
  );
}
