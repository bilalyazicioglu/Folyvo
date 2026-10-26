import { demoHandlers } from "./demo";

test("the demo backend behaves like the real one for the main loop", () => {
  const h = demoHandlers();
  const recent = h.recent({}) as { itemId: string; latestSeq: number }[];
  expect(recent[0].itemId).toBe("itm_demo1");

  const history = h.history({ item: "itm_demo1" }) as { id: string; seq: number }[];
  const latestBefore = history[0].seq;
  expect(latestBefore).toBe(recent[0].latestSeq);

  const copy = h.export({ item: "itm_demo1", version: history[2].id, destDir: "/tmp" }) as { versionId: string };
  expect(copy.versionId).toBe(history[2].id);

  h.restore({ item: "itm_demo1", version: history[3].id });
  const after = h.history({ item: "itm_demo1" }) as { seq: number }[];
  expect(after[0].seq).toBe(latestBefore + 1);
});

test("the demo shows a copy that left and a picture for each kind it can draw", () => {
  const h = demoHandlers();
  const copies = h.copies({ item: "itm_demo1" }) as { versionId: string; service: string }[];
  expect(copies).toEqual([expect.objectContaining({ versionId: "ver_08", service: "mail" })]);
  expect((h.preview({ item: "itm_demo1", version: "ver_09" }) as { picture: string }).picture).toMatch(/^data:image\//);
  expect((h.preview({ item: "itm_demo3", version: "ver_02" }) as { picture: null }).picture).toBeNull();
});
