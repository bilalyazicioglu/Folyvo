import type { QueryClient } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import type { EngineEventDto } from "./generated/EngineEventDto";

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
      void qc.invalidateQueries();
    });
  });
  return unlisten;
}
