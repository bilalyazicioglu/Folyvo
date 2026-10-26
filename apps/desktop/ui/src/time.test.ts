import { dayBounds, hourBuckets, middleTruncate, needsSeconds, whenLabel } from "./time";

const at = (d: number, h: number, m: number, s = 0) => new Date(2026, 9, d, h, m, s).getTime();
const now = at(26, 10, 42); // Monday 26 October 2026

test("today and yesterday and weekday and date words", () => {
  expect(whenLabel(at(26, 10, 12), now)).toBe("Today 10:12");
  expect(whenLabel(at(25, 17, 20), now)).toBe("Yesterday 17:20");
  expect(whenLabel(at(21, 10, 12), now)).toBe("Wednesday 10:12");
  expect(whenLabel(at(19, 9, 3), now)).toBe("19 Oct 09:03");
  expect(whenLabel(at(26, 10, 12, 8), now, true)).toBe("Today 10:12:08");
});

test("seconds only when two saves share a minute", () => {
  const a = at(26, 10, 12, 8);
  const b = at(26, 10, 12, 41);
  const c = at(26, 10, 31);
  expect(needsSeconds([a, b, c])).toEqual(new Set([a, b]));
});

test("buckets span eight to nineteen and stretch for late saves", () => {
  const b = hourBuckets([{ at: at(26, 9, 20) }, { at: at(26, 9, 58) }, { at: at(26, 10, 12) }], now);
  expect(b[0].hour).toBe(8);
  expect(b[b.length - 1].hour).toBe(19);
  expect(b.find((x) => x.hour === 9)!.count).toBe(2);
  const late = hourBuckets([{ at: at(26, 6, 5) }], at(26, 22, 0));
  expect([late[0].hour, late[late.length - 1].hour]).toEqual([6, 22]);
});

test("yesterday's 23:59 save is not today", () => {
  const { from, to } = dayBounds(now);
  expect([from, to]).toEqual([at(26, 0, 0), at(27, 0, 0)]);
  const b = hourBuckets([{ at: at(25, 23, 59) }], now);
  expect(b.reduce((n, x) => n + x.count, 0)).toBe(0);
});

test("long turkish names are cut in the middle keeping the extension", () => {
  expect(middleTruncate("Teklif_Müşteri_2026_son_hâli.docx", 30)).toBe("Teklif_Müşteri_2026_…hâli.docx");
  expect(middleTruncate("Budget_2027.xlsx", 30)).toBe("Budget_2027.xlsx");
});
