import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { mockIPC } from "@tauri-apps/api/mocks";
import { render } from "@testing-library/react";
import type { ReactElement } from "react";
import type { FolderDto } from "./generated/FolderDto";
import type { ItemSummaryDto } from "./generated/ItemSummaryDto";
import type { VersionDto } from "./generated/VersionDto";

export function item(over: Partial<ItemSummaryDto> = {}): ItemSummaryDto {
  return {
    itemId: "itm_01",
    name: "Proposal_Acme.docx",
    path: "/Users/a/Client work/Proposal_Acme.docx",
    status: "notShared",
    latestSeq: 1,
    latestAt: Date.now() - 120_000,
    missing: false,
    ...over,
  };
}

export function version(over: Partial<VersionDto> = {}): VersionDto {
  const seq = over.seq ?? 1;
  return {
    id: `ver_${seq.toString(16).padStart(2, "0")}`,
    seq,
    createdAt: Date.now() - seq * 60_000,
    size: 48_000,
    contentHash: "b3_00",
    origin: "auto",
    note: null,
    isShareReady: false,
    ...over,
  };
}

export interface Backend {
  recent: ItemSummaryDto[];
  history: Record<string, VersionDto[]>;
  folders: FolderDto[];
  stats: { items: number; versions: number; bytes: number };
  /** Per-command overrides; throw an object to simulate a typed backend error. */
  handlers: Record<string, (args: Record<string, unknown>) => unknown>;
  picked: string | null;
}

export function mockBackend(over: Partial<Backend> = {}) {
  const state: Backend = {
    recent: [],
    history: {},
    folders: [{ path: "/Users/a/Client work", problem: null }],
    stats: { items: 0, versions: 0, bytes: 0 },
    handlers: {},
    picked: null,
    ...over,
  };
  const calls: { cmd: string; args: Record<string, unknown> }[] = [];
  mockIPC(
    (cmd, payload) => {
      const args = (payload ?? {}) as Record<string, unknown>;
      calls.push({ cmd, args });
      if (state.handlers[cmd]) return state.handlers[cmd](args);
      switch (cmd) {
        case "recent":
          return state.recent;
        case "history":
          return state.history[args.item as string] ?? [];
        case "folders":
          return state.folders;
        case "stats":
          return state.stats;
        case "plugin:dialog|open":
          return state.picked;
        default:
          return undefined;
      }
    },
    { shouldMockEvents: true },
  );
  return { state, calls, called: (cmd: string) => calls.filter((c) => c.cmd === cmd) };
}

export function renderWithClient(ui: ReactElement) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: 0 } } });
  return render(<QueryClientProvider client={client}>{ui}</QueryClientProvider>);
}
