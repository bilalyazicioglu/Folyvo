import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import type { CopyDto } from "./generated/CopyDto";
import type { ItemSummaryDto } from "./generated/ItemSummaryDto";
import type { VersionDto } from "./generated/VersionDto";

type Args = Record<string, unknown>;
const MIN = 60_000;

/** A plain drawn page standing in for a Quick Look picture. */
const PAGE =
  "data:image/svg+xml;utf8," +
  encodeURIComponent(
    `<svg xmlns="http://www.w3.org/2000/svg" width="612" height="792"><rect width="612" height="792" fill="#fff"/>` +
      `<rect x="72" y="80" width="300" height="22" fill="#1d1d1f"/>` +
      Array.from({ length: 22 }, (_, i) => `<rect x="72" y="${140 + i * 26}" width="${420 + ((i * 37) % 48)}" height="9" fill="#c9cad0"/>`).join("") +
      `</svg>`,
  );

/** An in-memory stand-in for the Rust backend, used for design review and screenshots (`?demo=1`). */
export function demoHandlers() {
  const now = Date.now();
  const v = (seq: number, minutesAgo: number, size: number, extra: Partial<VersionDto> = {}): VersionDto => ({
    id: `ver_${seq.toString(16).padStart(2, "0")}`,
    code: seq.toString(16).padStart(2, "0"),
    seq,
    createdAt: now - minutesAgo * MIN,
    size,
    contentHash: `b3_${seq}`,
    origin: "auto",
    note: null,
    ...extra,
  });
  const histories: Record<string, VersionDto[]> = {
    itm_demo1: [
      v(9, 2, 412_000),
      v(8, 95, 398_000, { note: "Before the Acme call" }),
      v(7, 140, 301_000),
      v(6, 300, 196_000, { note: "Pricing section added" }),
      v(5, 1_500, 88_000),
      v(1, 2_900, 12_000, { origin: "initial" }),
    ],
    itm_demo2: [v(4, 60, 2_400_000), v(3, 400, 2_100_000), v(1, 3_000, 900_000, { origin: "initial" })],
    itm_demo3: [v(2, 1_440, 6_800_000), v(1, 4_400, 6_500_000, { origin: "initial" })],
  };
  const names: Record<string, string> = { itm_demo1: "Proposal_Acme.docx", itm_demo2: "Budget_2027.xlsx", itm_demo3: "Pitch_deck.pptx" };
  const recentOf = (): ItemSummaryDto[] =>
    Object.keys(histories).map((id) => {
      const h = histories[id];
      return { itemId: id, name: names[id], path: `/Users/you/Client work/${names[id]}`, latestSeq: h[0].seq, latestAt: h[0].createdAt, missing: false };
    });
  const byId = (a: Args) => histories[a.item as string] ?? [];
  const copies: CopyDto[] = [
    { itemId: "itm_demo1", versionId: "ver_08", kind: "handed", service: "mail", targetPath: null, at: now - 90 * MIN },
  ];

  return {
    recent: (_: Args) => recentOf(),
    history: (a: Args) => byId(a),
    stats: (_: Args) => ({ items: 3, versions: 17, bytes: 21_400_000 }),
    folders: (_: Args) => [{ path: "/Users/you/Client work", problem: null }],
    name_version: (a: Args) => {
      for (const h of Object.values(histories)) h.forEach((x) => { if (x.id === a.version) x.note = (a.note as string | null) ?? null; });
    },
    activity: (a: Args) => ({
      saves: Object.entries(histories).flatMap(([itemId, h]) =>
        h.filter((v) => v.createdAt >= (a.from as number) && v.createdAt < (a.to as number)).map((v) => ({ itemId, versionId: v.id, at: v.createdAt })),
      ),
      copies: copies.filter((c) => c.at >= (a.from as number) && c.at < (a.to as number)),
    }),
    copies: (a: Args) => copies.filter((c) => c.itemId === a.item),
    preview: (a: Args) => ({
      picture: a.item === "itm_demo3" ? null : PAGE,
      appName: a.item === "itm_demo2" ? "Microsoft Excel" : a.item === "itm_demo3" ? null : "Microsoft Word",
    }),
    item_at: (_: Args) => null,
    which: (_: Args) => [],
    export: (a: Args) => {
      return { destination: `${a.destDir}/${names[a.item as string]}`, versionId: a.version as string };
    },
    restore: (a: Args) => {
      const h = byId(a);
      const source = h.find((x) => x.id === a.version)!;
      const seq = h[0].seq + 1;
      h.unshift({ ...source, id: `ver_${seq.toString(16).padStart(2, "0")}`, seq, createdAt: Date.now(), origin: "restore", note: null });
      return { versionId: h[0].id, seq };
    },
    "plugin:dialog|open": (_: Args) => "/Users/you/Desktop",
    open_history: (_: Args) => undefined,
    hide_popover: (_: Args) => undefined,
    fit_popover: (_: Args) => undefined,
    open_version: (_: Args) => undefined,
    set_dialog_open: (_: Args) => undefined,
    reveal: (_: Args) => undefined,
    add_folder: (_: Args) => undefined,
    remove_folder: (_: Args) => undefined,
  } satisfies Record<string, (a: Args) => unknown>;
}

export function installDemo() {
  const handlers = demoHandlers() as Record<string, (a: Args) => unknown>;
  mockIPC((cmd, payload) => handlers[cmd]?.((payload ?? {}) as Args), { shouldMockEvents: true });
  mockWindows("demo");
}
