import type { VaultStatsDto } from "./generated/VaultStatsDto";
import type { VersionDto } from "./generated/VersionDto";

export const MIN_BAND = 28;
export const MAX_BAND = 56;
const LOW = Math.log2(1024);
const HIGH = Math.log2(2 ** 30);

/** Layer thickness in pixels: file size on a log scale, 1 KB and below thinnest, 1 GB and above thickest. */
export function bandHeight(size: number): number {
  const t = (Math.log2(Math.max(size, 1)) - LOW) / (HIGH - LOW);
  return Math.round(MIN_BAND + Math.min(1, Math.max(0, t)) * (MAX_BAND - MIN_BAND));
}

export function ago(thenMs: number, nowMs: number): string {
  const s = Math.max(0, Math.floor((nowMs - thenMs) / 1000));
  if (s < 45) return "just now";
  if (s < 3_600) return `${Math.max(1, Math.floor(s / 60))} min ago`;
  if (s < 86_400) return `${Math.floor(s / 3_600)} h ago`;
  const days = Math.floor(s / 86_400);
  return days === 1 ? "Yesterday" : `${days} days ago`;
}

export function sizeLabel(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
}

export function noteOf(v: VersionDto): string {
  if (v.note) return v.note;
  return v.origin === "initial" ? "First version" : v.origin === "restore" ? "Restored" : "Autosaved";
}

export const exportLabel = (seq: number) => `Export v${seq}`;

export type Band = "latest" | "older-a" | "older-b";

export function bandOf(_v: VersionDto, index: number): Band {
  if (index === 0) return "latest";
  return index % 2 === 1 ? "older-a" : "older-b";
}

export function folderName(path: string): string {
  const parts = path.split("/").filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

export function storedLabel(s: VaultStatsDto): string {
  return `Stored on this Mac, ${sizeLabel(s.bytes)}, ${s.versions} ${s.versions === 1 ? "version" : "versions"}`;
}
