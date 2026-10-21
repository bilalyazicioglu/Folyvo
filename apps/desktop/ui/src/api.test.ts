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
  expect(seen).toEqual({ item: "itm_01", expected: "ver_01", destDir: "/Sent" });
  expect(out.destination).toBe("/Sent/a.docx");
});

test("a typed backend error becomes an AppError with its title and hint", async () => {
  mockIPC(() => {
    throw { code: "share_ready_changed", title: "The ready version changed", hint: "Look at the current ready version and export again." };
  });
  const err = await api.markReady("itm_01", "ver_01").catch((e) => e);
  expect(err).toBeInstanceOf(AppError);
  expect(err.info.code).toBe("share_ready_changed");
  expect(err.info.hint).toContain("export again");
});

test("anything that is not a typed error becomes the generic message", async () => {
  mockIPC(() => {
    throw "boom: sqlite exploded";
  });
  const err = await api.stats().catch((e) => e);
  expect(err.info).toEqual(INTERNAL_ERROR);
  expect(JSON.stringify(err.info)).not.toContain("sqlite");
});
