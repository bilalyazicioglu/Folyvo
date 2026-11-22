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

// Measuring the room taken and what the rule would let go reads the whole vault: a save does not
// make them stale, opening the window does.
test("an event refreshes everything but what reads the whole vault", async () => {
  mockIPC(() => undefined, { shouldMockEvents: true });
  const frames: FrameRequestCallback[] = [];
  vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => frames.push(cb));
  const qc = new QueryClient();
  for (const key of [["recent"], ["space", 0], ["letting-go"]]) qc.setQueryData(key, 1);
  const stop = await startEventBridge(qc);

  await emit("engine-event", { kind: "versionCreated", itemId: "itm_01", versionId: "ver_01" });
  frames[0](0);
  expect(qc.getQueryState(["recent"])?.isInvalidated).toBe(true);
  expect(qc.getQueryState(["space", 0])?.isInvalidated).toBe(false);
  expect(qc.getQueryState(["letting-go"])?.isInvalidated).toBe(false);
  stop();
  vi.unstubAllGlobals();
});
