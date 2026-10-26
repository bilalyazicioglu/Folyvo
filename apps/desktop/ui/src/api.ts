import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type { ActivityDto } from "./generated/ActivityDto";
import type { CopyDto } from "./generated/CopyDto";
import type { ExportDto } from "./generated/ExportDto";
import type { MatchDto } from "./generated/MatchDto";
import type { PreviewDto } from "./generated/PreviewDto";
import type { FolderDto } from "./generated/FolderDto";
import type { ItemSummaryDto } from "./generated/ItemSummaryDto";
import type { RestoreDto } from "./generated/RestoreDto";
import type { UserError } from "./generated/UserError";
import type { VaultStatsDto } from "./generated/VaultStatsDto";
import type { VersionDto } from "./generated/VersionDto";

export const INTERNAL_ERROR: UserError = {
  code: "internal",
  title: "Something went wrong inside Folyvo",
  hint: "Nothing was changed. Try again.",
};

export class AppError extends Error {
  constructor(readonly info: UserError) {
    super(info.title);
  }
}

function isUserError(e: unknown): e is UserError {
  return typeof e === "object" && e !== null && ["code", "title", "hint"].every((k) => typeof (e as Record<string, unknown>)[k] === "string");
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw new AppError(isUserError(e) ? e : INTERNAL_ERROR);
  }
}

export const api = {
  recent: (limit = 20) => call<ItemSummaryDto[]>("recent", { limit }),
  history: (item: string) => call<VersionDto[]>("history", { item }),
  nameVersion: (version: string, note: string | null) => call<void>("name_version", { version, note }),
  exportVersion: (item: string, version: string, destDir: string) => call<ExportDto>("export", { item, version, destDir }),
  /** Saves and copies with from <= time < to (unix ms). */
  activity: (from: number, to: number) => call<ActivityDto>("activity", { from, to }),
  copies: (item: string) => call<CopyDto[]>("copies", { item }),
  itemAt: (path: string) => call<string | null>("item_at", { path }),
  which: (path: string) => call<MatchDto[]>("which", { path }),
  preview: (item: string, version: string) => call<PreviewDto>("preview", { item, version }),
  /** Opens the working file (newest) or a read-only copy of the version in its app. */
  openVersion: (item: string, version: string, newest: boolean) => call<void>("open_version", { item, version, newest }),
  restore: (item: string, version: string) => call<RestoreDto>("restore", { item, version }),
  stats: () => call<VaultStatsDto>("stats"),
  folders: () => call<FolderDto[]>("folders"),
  addFolder: (path: string) => call<void>("add_folder", { path }),
  removeFolder: (path: string) => call<void>("remove_folder", { path }),
  openHistory: (item: string) => call<void>("open_history", { item }),
  hidePopover: () => call<void>("hide_popover"),
  reveal: (path: string) => call<void>("reveal", { path }),
  /** Native folder picker. Resolves to null when the person cancels. */
  chooseFolder: async (title: string): Promise<string | null> => {
    // The picker takes focus from the popover, which would hide it and the picker with it.
    await call<void>("set_dialog_open", { open: true });
    try {
      const picked = await open({ directory: true, multiple: false, title });
      return typeof picked === "string" ? picked : null;
    } finally {
      await call<void>("set_dialog_open", { open: false });
    }
  },
};
