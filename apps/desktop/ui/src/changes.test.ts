import { changeSummary } from "./changes";
import type { ChangeCountDto } from "./generated/ChangeCountDto";

const n = (unit: ChangeCountDto["unit"], added = 0, removed = 0, changed = 0): ChangeCountDto => ({
  unit,
  added,
  removed,
  changed,
});
const text = (...counts: ChangeCountDto[]) => changeSummary({ outcome: "text", counts });

test("one kind of change in one unit reads as a count", () => {
  expect(text(n("paragraph", 0, 0, 2))).toBe("2 paragraphs changed");
  expect(text(n("cell", 0, 0, 1))).toBe("1 cell changed");
  expect(text(n("sheet", 1))).toBe("1 sheet added");
  expect(text(n("page", 0, 0, 1))).toBe("1 page changed");
  expect(text(n("line", 0, 3))).toBe("3 lines removed");
});

test("several kinds in one unit share the unit's name", () => {
  expect(text(n("paragraph", 1, 1))).toBe("1 paragraph added, 1 removed");
  expect(text(n("paragraph", 2, 0, 1))).toBe("1 paragraph changed, 2 added");
});

test("two units changed the same way are named together", () => {
  expect(text(n("paragraph", 0, 0, 2), n("cell", 0, 0, 1))).toBe("2 paragraphs and 1 cell changed");
});

test("different changes in two units are told one after the other", () => {
  expect(text(n("paragraph", 1), n("cell", 0, 0, 3))).toBe("3 cells changed, 1 paragraph added");
});

test("only the two units with the most changes are named", () => {
  expect(text(n("line", 0, 0, 1), n("paragraph", 0, 0, 5), n("cell", 0, 0, 2))).toBe(
    "5 paragraphs and 2 cells changed",
  );
});

test("the other outcomes say plainly what is known", () => {
  expect(changeSummary({ outcome: "sameText", counts: [] })).toBe("Same text, other changes");
  expect(changeSummary({ outcome: "identical", counts: [] })).toBe("Same as the save before");
  expect(changeSummary({ outcome: "unknown", counts: [] })).toBe("Changed");
  expect(changeSummary({ outcome: "first", counts: [] })).toBeNull();
  expect(changeSummary(null)).toBeNull();
});
