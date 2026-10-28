import type { ChangeCountDto } from "./generated/ChangeCountDto";
import type { ChangeDto } from "./generated/ChangeDto";

const UNIT: Record<ChangeCountDto["unit"], [string, string]> = {
  line: ["line", "lines"],
  paragraph: ["paragraph", "paragraphs"],
  cell: ["cell", "cells"],
  sheet: ["sheet", "sheets"],
  page: ["page", "pages"],
};

const counted = (n: number, unit: ChangeCountDto["unit"]) => `${n} ${UNIT[unit][n === 1 ? 0 : 1]}`;

/** A unit's changes as (count, verb), the kinds that happened, changed first. */
const verbs = (c: ChangeCountDto): [number, string][] =>
  (
    [
      [c.changed, "changed"],
      [c.added, "added"],
      [c.removed, "removed"],
    ] as [number, string][]
  ).filter(([n]) => n > 0);

/**
 * What changed in a save, in words that are literally true: "2 paragraphs changed",
 * "1 paragraph added, 1 removed", "2 paragraphs and 1 cell changed". At most the two units with
 * the most changes are named. `null` when there is nothing to say (the first save, or not known yet).
 */
export function changeSummary(change: ChangeDto | null): string | null {
  if (!change) return null;
  switch (change.outcome) {
    case "first":
      return null;
    case "sameText":
      return "Same text, other changes";
    case "identical":
      return "Same as the save before";
    case "unknown":
      return "Changed";
  }
  const total = (c: ChangeCountDto) => c.added + c.removed + c.changed;
  const units = [...change.counts]
    .filter((c) => total(c) > 0)
    .sort((a, b) => total(b) - total(a))
    .slice(0, 2);
  if (units.length === 0) return "Changed";
  const [a, b] = units.map(verbs);
  if (b && a.length === 1 && b.length === 1 && a[0][1] === b[0][1]) {
    return `${counted(a[0][0], units[0].unit)} and ${counted(b[0][0], units[1].unit)} ${a[0][1]}`;
  }
  return units
    .map((u, i) => {
      const [[first, verb], ...rest] = i === 0 ? a : b;
      return [`${counted(first, u.unit)} ${verb}`, ...rest.map(([m, v]) => `${m} ${v}`)].join(", ");
    })
    .join(", ");
}
