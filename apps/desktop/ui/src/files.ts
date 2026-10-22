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
