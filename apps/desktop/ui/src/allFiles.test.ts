import { groupFiles, matchesName } from "./allFiles";
import { item } from "./test-utils";

describe("finding a file by part of its name", () => {
  it("ignores case and matches anywhere in the name", () => {
    expect(matchesName("Bilal_Resume-3.pdf", "resume-3")).toBe(true);
    expect(matchesName("Bilal_Resume-3.pdf", "RESUME")).toBe(true);
    expect(matchesName("Bilal_Resume-3.pdf", "budget")).toBe(false);
  });

  it("finds a Turkish name typed without its letters", () => {
    expect(matchesName("Teklif_Müşteri.docx", "musteri")).toBe(true);
    expect(matchesName("BÜTÇE_İLK.xlsx", "butce")).toBe(true);
    expect(matchesName("BÜTÇE_İLK.xlsx", "ilk")).toBe(true);
    expect(matchesName("Kısa_not.txt", "kisa")).toBe(true);
  });

  it("finds a name Finder stored in decomposed form", () => {
    expect(matchesName("Müşteri.docx", "Müşteri")).toBe(true);
  });

  it("takes spaces around what was typed as nothing, and an empty search as everything", () => {
    expect(matchesName("a.docx", "  a  ")).toBe(true);
    expect(matchesName("a.docx", "   ")).toBe(true);
  });
});

describe("files by folder", () => {
  const folders = [{ path: "/Users/a/Client work" }, { path: "/Users/a/Client work/Old" }, { path: "/Users/a/School" }];

  it("puts each file under the deepest folder it is in, newest first", () => {
    const groups = groupFiles(
      [
        item({ itemId: "itm_1", name: "a.docx", path: "/Users/a/Client work/a.docx", latestAt: 1 }),
        item({ itemId: "itm_2", name: "b.docx", path: "/Users/a/Client work/b.docx", latestAt: 2 }),
        item({ itemId: "itm_3", name: "c.docx", path: "/Users/a/Client work/Old/c.docx", latestAt: 3 }),
        item({ itemId: "itm_4", name: "d.docx", path: "/Users/a/School/d.docx", latestAt: 4 }),
      ],
      folders,
    );
    expect(groups.map((g) => [g.name, g.files.map((f) => f.item.itemId)])).toEqual([
      ["Client work", ["itm_2", "itm_1"]],
      ["Old", ["itm_3"]],
      ["School", ["itm_4"]],
    ]);
  });

  it("names the subfolder a file is in", () => {
    const [group] = groupFiles(
      [
        item({ name: "a.docx", path: "/Users/a/School/Spring/Essays/a.docx" }),
        item({ itemId: "itm_2", name: "b.docx", path: "/Users/a/School/b.docx" }),
      ],
      folders,
    );
    expect(group.files.map((f) => f.subfolder)).toEqual(expect.arrayContaining([null, "Spring/Essays"]));
  });

  it("does not take a folder that only starts the same for the file's folder", () => {
    const [group] = groupFiles([item({ path: "/Users/a/School trips/x.docx", name: "x.docx" })], folders);
    expect(group.path).toBeNull();
  });

  it("keeps the files of a folder no longer watched, and files without a path, last", () => {
    const groups = groupFiles(
      [
        item({ itemId: "itm_1", path: "/Users/a/Gone/x.docx", name: "x.docx", latestAt: 9 }),
        item({ itemId: "itm_2", path: null, name: "y.docx", latestAt: 8 }),
        item({ itemId: "itm_3", path: "/Users/a/School/z.docx", name: "z.docx", latestAt: 1 }),
      ],
      folders,
    );
    expect(groups.map((g) => g.name)).toEqual(["School", "Other files"]);
    expect(groups[1].files.map((f) => f.item.itemId)).toEqual(["itm_1", "itm_2"]);
  });

  it("leaves out a folder with no files in it", () => {
    expect(groupFiles([], folders)).toEqual([]);
  });
});
