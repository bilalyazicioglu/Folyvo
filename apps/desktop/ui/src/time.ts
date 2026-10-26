/** Time in the words people use, and the day's shape for the Today strip. All local time. */

const DAY = 86_400_000;
const WEEKDAYS = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

const pad = (n: number) => String(n).padStart(2, "0");

function startOfDay(t: number): number {
  const d = new Date(t);
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
}

/** Local midnight to the next local midnight around `now`. */
export function dayBounds(now: number): { from: number; to: number } {
  const from = startOfDay(now);
  const d = new Date(from);
  return { from, to: new Date(d.getFullYear(), d.getMonth(), d.getDate() + 1).getTime() };
}

export function clock(at: number, withSeconds = false): string {
  const d = new Date(at);
  return `${pad(d.getHours())}:${pad(d.getMinutes())}${withSeconds ? `:${pad(d.getSeconds())}` : ""}`;
}

/** "Today 10:12", "Yesterday 17:20", "Wednesday 10:12" within six days, else "12 Oct 10:12". */
export function whenLabel(at: number, now: number, withSeconds = false): string {
  const days = Math.round((startOfDay(now) - startOfDay(at)) / DAY);
  const d = new Date(at);
  const day =
    days <= 0 ? "Today" : days === 1 ? "Yesterday" : days <= 6 ? WEEKDAYS[d.getDay()] : `${d.getDate()} ${MONTHS[d.getMonth()]}`;
  return `${day} ${clock(at, withSeconds)}`;
}

/** The times that share a minute with another, so they need seconds to be told apart. */
export function needsSeconds(times: number[]): Set<number> {
  const byMinute = new Map<number, number[]>();
  for (const t of times) {
    const m = Math.floor(t / 60_000);
    byMinute.set(m, [...(byMinute.get(m) ?? []), t]);
  }
  return new Set([...byMinute.values()].filter((ts) => ts.length > 1).flat());
}

/** Saves per hour today, from 08 (or the first save) to 19 (or now). */
export function hourBuckets(saves: { at: number }[], now: number): { hour: number; count: number }[] {
  const { from, to } = dayBounds(now);
  const today = saves.filter((s) => s.at >= from && s.at < to).map((s) => new Date(s.at).getHours());
  const first = Math.min(8, ...today);
  const last = Math.max(19, new Date(now).getHours(), ...today);
  const out: { hour: number; count: number }[] = [];
  for (let hour = first; hour <= last; hour++) out.push({ hour, count: today.filter((h) => h === hour).length });
  return out;
}

/** Cut a long file name in the middle, keeping the extension and the four characters before it. */
export function middleTruncate(name: string, max: number): string {
  const chars = [...name];
  if (chars.length <= max) return name;
  const dot = name.lastIndexOf(".");
  const tailStart = dot > 4 ? [...name.slice(0, dot)].length - 4 : chars.length - 8;
  const tail = chars.slice(tailStart);
  return chars.slice(0, max - tail.length - 1).join("") + "…" + tail.join("");
}
