export type RelationshipTargetRow = {
  id: string;
  name: string;
  relationship: string;
  notes: string;
};

type StoredRelationshipTarget = {
  name?: unknown;
  relationship?: unknown;
  notes?: unknown;
};

function toText(value: unknown) {
  return typeof value === "string" ? value : "";
}

function emptyRow(): RelationshipTargetRow {
  return {
    id: "relationship-target-empty",
    name: "",
    relationship: "",
    notes: ""
  };
}

export function parseRelationshipTargets(value: string): RelationshipTargetRow[] {
  let parsed: unknown;
  try {
    parsed = JSON.parse(value || "[]");
  } catch {
    parsed = [];
  }
  if (!Array.isArray(parsed)) return [emptyRow()];
  const rows = parsed
    .map((entry: StoredRelationshipTarget, index) => {
      const name = toText(entry?.name).trim();
      const relationship = toText(entry?.relationship).trim();
      const notes = toText(entry?.notes).trim();
      if (!name && !relationship && !notes) return null;
      return {
        id: `relationship-target-${index}-${name || "empty"}`,
        name,
        relationship,
        notes
      };
    })
    .filter((row): row is RelationshipTargetRow => Boolean(row));
  return rows.length ? rows : [emptyRow()];
}

export function serializeRelationshipTargets(rows: RelationshipTargetRow[]) {
  return JSON.stringify(
    rows
      .map((row) => ({
        name: row.name.trim(),
        relationship: row.relationship.trim(),
        notes: row.notes.trim()
      }))
      .filter((row) => row.name)
  );
}
