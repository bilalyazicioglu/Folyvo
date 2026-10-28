import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { api, AppError, INTERNAL_ERROR } from "./api";
import type { UserError } from "./generated/UserError";

export const useRecent = () => useQuery({ queryKey: ["recent"], queryFn: () => api.recent(200) });
export const useItem = (item: string) => useQuery({ queryKey: ["item", item], queryFn: () => api.item(item) });
export const useHistory = (item?: string) =>
  useQuery({ queryKey: ["history", item], queryFn: () => api.history(item!), enabled: !!item });
export const useStats = () => useQuery({ queryKey: ["stats"], queryFn: api.stats });
export const useFolders = () => useQuery({ queryKey: ["folders"], queryFn: api.folders });
export const useActivity = (from: number, to: number) =>
  useQuery({ queryKey: ["activity", from, to], queryFn: () => api.activity(from, to) });
/** A version's picture never changes, so it is drawn once per window. */
export const usePreview = (item: string, version?: string) =>
  useQuery({ queryKey: ["preview", item, version], queryFn: () => api.preview(item, version!), enabled: !!version, staleTime: Infinity, retry: false });
/** A save's text with what changed marked. Asked for only while it is on screen; it never changes. */
export const useChanges = (item: string, version: string | undefined, enabled: boolean) =>
  useQuery({
    queryKey: ["changes", item, version],
    queryFn: () => api.changes(item, version!),
    enabled: enabled && !!version,
    staleTime: Infinity,
    retry: false,
  });
export const useCopies = (item?: string) =>
  useQuery({ queryKey: ["copies", item], queryFn: () => api.copies(item!), enabled: !!item });

/** Re-renders every half minute so "2 min ago" stays true. */
export function useNow(): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), 30_000);
    return () => clearInterval(id);
  }, []);
  return now;
}

/** Runs one user action: tracks busy and error state and refreshes everything afterwards. */
export function useAction() {
  const qc = useQueryClient();
  const [error, setError] = useState<UserError | null>(null);
  const [busy, setBusy] = useState(false);
  const run = async <T,>(fn: () => Promise<T>): Promise<{ ok: true; value: T } | { ok: false }> => {
    setBusy(true);
    setError(null);
    try {
      const value = await fn();
      await qc.invalidateQueries();
      return { ok: true, value };
    } catch (e) {
      setError(e instanceof AppError ? e.info : INTERNAL_ERROR);
      await qc.invalidateQueries();
      return { ok: false };
    } finally {
      setBusy(false);
    }
  };
  return { run, error, busy, clearError: () => setError(null) };
}
