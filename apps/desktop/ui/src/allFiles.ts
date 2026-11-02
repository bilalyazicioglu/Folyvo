import { folderName } from "./files";
import type { ItemSummaryDto } from "./generated/ItemSummaryDto";

/** Letters as people type them: no case, no marks, Turkish dotless i as i. Finder may store names decomposed. */
function plain(text: string): string {
  return text
    .toLocaleLowerCase("en")
    .normalize("NFD")
    .replace(/\p{M}/gu, "")
    .replace(/ı/g, "i");
}

/** Whether a file's name holds what was typed. An empty search holds everything. */
export function matchesName(name: string, typed: string): boolean {
  return plain(name).includes(plain(typed.trim()));
}

export interface FileEntry {
  item: ItemSummaryDto;
  /** Where the file is below its kept folder ("Spring/Essays"); null when directly in it. */
  subfolder: string | null;
}

export interface FileGroup {
  /** The kept folder; null for files outside any folder kept now. */
  path: string | null;
  name: string;
  files: FileEntry[];
}

const OTHER = "Other files";

function isInside(file: string, folder: string): boolean {
  return file.startsWith(`${folder.replace(/\/+$/, "")}/`);
}

function subfolderOf(file: string, folder: string): string | null {
  const below = file.slice(folder.replace(/\/+$/, "").length + 1);
  const end = below.lastIndexOf("/");
  return end > 0 ? below.slice(0, end) : null;
}

/** Files under the deepest kept folder holding them, newest first; files outside all of them come last. */
export function groupFiles(items: ItemSummaryDto[], folders: { path: string }[]): FileGroup[] {
  const deepestFirst = [...folders].sort((a, b) => b.path.length - a.path.length);
  const byFolder = new Map<string | null, FileEntry[]>();
  for (const item of [...items].sort((a, b) => b.latestAt - a.latestAt)) {
    const home = item.path === null ? undefined : deepestFirst.find((f) => isInside(item.path!, f.path));
    const key = home?.path ?? null;
    const entry = { item, subfolder: home ? subfolderOf(item.path!, home.path) : null };
    byFolder.set(key, [...(byFolder.get(key) ?? []), entry]);
  }
  const kept = folders.flatMap((f) => {
    const files = byFolder.get(f.path);
    return files ? [{ path: f.path, name: folderName(f.path), files }] : [];
  });
  const other = byFolder.get(null);
  return other ? [...kept, { path: null, name: OTHER, files: other }] : kept;
}
