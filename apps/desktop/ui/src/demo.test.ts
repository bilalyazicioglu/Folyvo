import { demoHandlers } from "./demo";

test("the demo backend behaves like the real one for the main loop", () => {
  const h = demoHandlers();
  const recent = h.recent({}) as { itemId: string; status: string; latestSeq: number }[];
  expect(recent[0].itemId).toBe("itm_demo1");
  expect(recent[0].status).toBe("changedSinceReady");

  const history = h.history({ item: "itm_demo1" }) as { id: string; seq: number; isShareReady: boolean }[];
  const latestBefore = history[0].seq;
  expect(latestBefore).toBe(recent[0].latestSeq);
  expect(history.filter((v) => v.isShareReady)).toHaveLength(1);

  h.mark_ready({ item: "itm_demo1", version: history[0].id });
  expect((h.recent({}) as { status: string }[])[0].status).toBe("ready");

  h.restore({ item: "itm_demo1", version: history[3].id });
  const after = h.history({ item: "itm_demo1" }) as { seq: number }[];
  expect(after[0].seq).toBe(latestBefore + 1);
});

test("exporting without a ready version is refused like in the real app", () => {
  const h = demoHandlers();
  expect(() => h.export({ item: "itm_demo2", expected: "ver_x", destDir: "/tmp" })).toThrow();
});
