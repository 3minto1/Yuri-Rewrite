export type AdditionalNameMappingRow = {
  id: string;
  source: string;
  target: string;
};

const mappingDelimiters = ["->", "=>", "→"];

function parseLine(value: string): Omit<AdditionalNameMappingRow, "id"> | null {
  const trimmed = value.trim();
  if (!trimmed) return null;
  for (const delimiter of mappingDelimiters) {
    const index = trimmed.indexOf(delimiter);
    if (index < 0) continue;
    const source = trimmed.slice(0, index).trim();
    const target = trimmed.slice(index + delimiter.length).trim();
    if (!source) return null;
    return { source, target: target === source ? "" : target };
  }
  return { source: trimmed, target: "" };
}

export function parseAdditionalNameMappings(value: string): AdditionalNameMappingRow[] {
  const rows = value
    .split(/[\n\r,，、;；]+/u)
    .map(parseLine)
    .filter((row): row is Omit<AdditionalNameMappingRow, "id"> => Boolean(row))
    .map((row, index) => ({ ...row, id: `additional-name-${index}-${row.source}` }));
  return rows.length ? rows : [{ id: "additional-name-empty", source: "", target: "" }];
}

export function serializeAdditionalNameMappings(rows: AdditionalNameMappingRow[]) {
  return rows
    .map((row) => {
      const source = row.source.trim();
      const target = row.target.trim();
      if (!source) return "";
      return target && target !== source ? `${source} -> ${target}` : source;
    })
    .filter(Boolean)
    .join("\n");
}

