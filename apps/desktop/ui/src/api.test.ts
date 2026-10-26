import { mockIPC } from "@tauri-apps/api/mocks";
import { api, AppError, INTERNAL_ERROR } from "./api";

test("commands pass camelCase arguments and return the backend's data", async () => {
  let seen: unknown;
  mockIPC((cmd, args) => {
    if (cmd === "export") {
      seen = args;
      return { destination: "/Sent/a.docx", versionId: "ver_01" };
    }
  });
  const out = await api.exportVersion("itm_01", "ver_01", "/Sent");
  expect(seen).toEqual({ item: "itm_01", version: "ver_01", destDir: "/Sent" });
  expect(out.destination).toBe("/Sent/a.docx");
});

test("a typed backend error becomes an AppError with its title and hint", async () => {
  mockIPC(() => {
    throw { code: "version_missing", title: "That version isn't there", hint: "Open the history again and pick another version." };
  });
  const err = await api.exportVersion("itm_01", "ver_01", "/tmp").catch((e) => e);
  expect(err).toBeInstanceOf(AppError);
  expect(err.info.code).toBe("version_missing");
  expect(err.info.hint).toContain("history again");
});

test("anything that is not a typed error becomes the generic message", async () => {
  mockIPC(() => {
    throw "boom: sqlite exploded";
  });
  const err = await api.stats().catch((e) => e);
  expect(err.info).toEqual(INTERNAL_ERROR);
  expect(JSON.stringify(err.info)).not.toContain("sqlite");
});

test("exportVersion sends the version to copy, not an expected one", async () => {
  let seen: unknown;
  mockIPC((cmd, args) => {
    if (cmd === "export") seen = args;
    return { destination: "/out/a.docx", versionId: "ver_01" };
  });
  await api.exportVersion("itm_01", "ver_01", "/out");
  expect(seen).toEqual({ item: "itm_01", version: "ver_01", destDir: "/out" });
});

test("which passes the path and returns the matches", async () => {
  let seen: unknown;
  mockIPC((cmd, args) => {
    if (cmd === "which") seen = args;
    return [];
  });
  expect(await api.which("/Users/a/back.docx")).toEqual([]);
  expect(seen).toEqual({ path: "/Users/a/back.docx" });
});
