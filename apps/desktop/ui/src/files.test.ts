import { fileKind, folderName, sizeLabel } from "./files";

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

test("sizes read like a person would say them", () => {
  expect(sizeLabel(512)).toBe("512 B");
  expect(sizeLabel(48_000)).toBe("47 KB");
  expect(sizeLabel(5 * 1024 * 1024)).toBe("5.0 MB");
  expect(sizeLabel(3 * 1024 ** 3)).toBe("3.0 GB");
});

test("a folder is named by its last part", () => {
  expect(folderName("/Users/a/Client work")).toBe("Client work");
  expect(folderName("/Users/a/Client work/")).toBe("Client work");
});
