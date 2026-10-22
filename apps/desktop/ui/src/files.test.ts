import { fileKind } from "./files";

test.each([
  ["Proposal.docx", "doc"],
  ["notes.md", "doc"],
  ["Budget 2026.XLSX", "sheet"],
  ["Pitch deck.pptx", "slides"],
  ["Logo final çıktı.pdf", "pdf"],
  ["photo.HEIC", "image"],
  ["archive.zip", "other"],
  ["README", "other"],
  [".hidden", "other"],
])("%s is a %s", (name, kind) => {
  expect(fileKind(name)).toBe(kind);
});
