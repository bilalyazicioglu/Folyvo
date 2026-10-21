import { version } from "./test-utils";
import { ago, bandHeight, bandOf, changedSince, exportLabel, folderName, markLabel, MAX_BAND, MIN_BAND, noteOf, sizeLabel, storedLabel } from "./strata";

test("thickness grows with file size on a log scale and stays inside its limits", () => {
  expect(bandHeight(0)).toBe(MIN_BAND);
  expect(bandHeight(1024)).toBe(MIN_BAND);
  expect(bandHeight(2 ** 30)).toBe(MAX_BAND);
  expect(bandHeight(2 ** 40)).toBe(MAX_BAND);
  const small = bandHeight(10 * 1024);
  const medium = bandHeight(10 * 1024 * 1024);
  expect(MIN_BAND < small && small < medium && medium < MAX_BAND).toBe(true);
  // log scale: 1 MB is about the middle of 1 KB .. 1 GB
  expect(Math.abs(bandHeight(2 ** 20) - (MIN_BAND + MAX_BAND) / 2)).toBeLessThanOrEqual(1);
});

test("relative times use the largest whole unit", () => {
  const now = 10_000_000_000;
  expect(ago(now - 10_000, now)).toBe("just now");
  expect(ago(now - 2 * 60_000, now)).toBe("2 min ago");
  expect(ago(now - 3 * 3_600_000, now)).toBe("3 h ago");
  expect(ago(now - 26 * 3_600_000, now)).toBe("Yesterday");
  expect(ago(now - 5 * 86_400_000, now)).toBe("5 days ago");
  expect(ago(now + 5_000, now)).toBe("just now");
});

test("sizes read like a person would say them", () => {
  expect(sizeLabel(512)).toBe("512 B");
  expect(sizeLabel(48_000)).toBe("47 KB");
  expect(sizeLabel(5 * 1024 * 1024)).toBe("5.0 MB");
});

test("a version without a name is described by how it came about", () => {
  expect(noteOf(version({ origin: "initial" }))).toBe("First version");
  expect(noteOf(version({ origin: "auto" }))).toBe("Autosaved");
  expect(noteOf(version({ origin: "restore" }))).toBe("Restored");
  expect(noteOf(version({ origin: "auto", note: "Sent to Acme" }))).toBe("Sent to Acme");
});

test("buttons and sentences name the version they are about", () => {
  expect(exportLabel(8)).toBe("Export v8");
  expect(markLabel(9)).toBe("Mark v9 ready");
  expect(changedSince(8)).toBe("Changed since v8, your share-ready version.");
});

test("band kinds: ready wins, then the latest, then alternating older layers", () => {
  expect(bandOf(version({ isShareReady: true }), 0)).toBe("ready");
  expect(bandOf(version({ isShareReady: true }), 3)).toBe("ready");
  expect(bandOf(version(), 0)).toBe("latest");
  expect(bandOf(version(), 1)).toBe("older-a");
  expect(bandOf(version(), 2)).toBe("older-b");
});

test("folder names and the stored-size line", () => {
  expect(folderName("/Users/a/Client work")).toBe("Client work");
  expect(folderName("/Users/a/Client work/")).toBe("Client work");
  expect(storedLabel({ items: 2, versions: 9, bytes: 3 * 1024 * 1024 })).toBe("Stored on this Mac, 3.0 MB, 9 versions");
  expect(storedLabel({ items: 1, versions: 1, bytes: 10 })).toBe("Stored on this Mac, 10 B, 1 version");
});
