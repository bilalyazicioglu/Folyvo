import { QueryClient } from "@tanstack/react-query";
import { emit } from "@tauri-apps/api/event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { startEventBridge } from "./events";

// Review Focus 6: a burst of events must not trigger a burst of refetches.
test("fifty events in a row cause one refetch on the next frame", async () => {
  mockIPC(() => undefined, { shouldMockEvents: true });
  const frames: FrameRequestCallback[] = [];
  vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => frames.push(cb));
  const qc = new QueryClient();
  const invalidate = vi.spyOn(qc, "invalidateQueries").mockResolvedValue();
  const stop = await startEventBridge(qc);

  for (let i = 0; i < 50; i++) await emit("engine-event", { kind: "versionCreated", itemId: "itm_01", versionId: "ver_01" });
  expect(frames).toHaveLength(1);
  expect(invalidate).not.toHaveBeenCalled();
  frames[0](0);
  expect(invalidate).toHaveBeenCalledTimes(1);

  await emit("engine-event", { kind: "rootsChanged" });
  expect(frames).toHaveLength(2);
  stop();
  vi.unstubAllGlobals();
});
