export type FileKind = "doc" | "sheet" | "slides" | "pdf" | "image" | "other";

const KINDS: Record<string, FileKind> = {
  doc: "doc", docx: "doc", pages: "doc", rtf: "doc", txt: "doc", md: "doc",
  xls: "sheet", xlsx: "sheet", numbers: "sheet", csv: "sheet",
  ppt: "slides", pptx: "slides", key: "slides",
  pdf: "pdf",
  png: "image", jpg: "image", jpeg: "image", gif: "image", heic: "image", svg: "image", psd: "image",
};

/** Coarse kind from the extension; the Popover draws one tile per kind. */
export function fileKind(name: string): FileKind {
  const dot = name.lastIndexOf(".");
  if (dot <= 0) return "other";
  return KINDS[name.slice(dot + 1).toLowerCase()] ?? "other";
}

/** The last part of a folder path, as Finder shows it. */
export function folderName(path: string): string {
  const parts = path.split("/").filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

export function sizeLabel(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
}
