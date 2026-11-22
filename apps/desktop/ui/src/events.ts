import type { QueryClient } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import type { EngineEventDto } from "./generated/EngineEventDto";

/**
 * Queries that read the whole vault. A save does not make them stale enough to read it all again:
 * they follow the window being opened or focused, and the person's own actions.
 */
const WHOLE_VAULT = new Set<unknown>(["space", "letting-go"]);

/**
 * Every engine event means "something the UI shows may be stale". Refetching once per animation
 * frame at most keeps a burst (many files saved at once) from freezing the window.
 */
export async function startEventBridge(qc: QueryClient): Promise<() => void> {
  let scheduled = false;
  const unlisten = await listen<EngineEventDto>("engine-event", () => {
    if (scheduled) return;
    scheduled = true;
    requestAnimationFrame(() => {
      scheduled = false;
      void qc.invalidateQueries({ predicate: (q) => !WHOLE_VAULT.has(q.queryKey[0]) });
    });
  });
  return unlisten;
}
