import type { SaveDto } from "../generated/SaveDto";
import { hourBuckets } from "../time";

const pad = (h: number) => String(h).padStart(2, "0");

function countLabel(saves: SaveDto[]): string {
  if (saves.length === 0) return "No saves yet today";
  const files = new Set(saves.map((s) => s.itemId)).size;
  return `${saves.length} ${saves.length === 1 ? "save" : "saves"} in ${files} ${files === 1 ? "file" : "files"}`;
}

/** "Today", how much was kept, and one bar per hour with the current hour in the accent colour. */
export function TodayStrip({ saves, now }: { saves: SaveDto[]; now: number }) {
  const buckets = hourBuckets(saves, now);
  const most = Math.max(1, ...buckets.map((b) => b.count));
  const busiest = buckets.reduce((a, b) => (b.count > a.count ? b : a), buckets[0]);
  const current = new Date(now).getHours();
  const label =
    busiest.count === 0 ? "Saves per hour today, none yet" : `Saves per hour today, most at ${pad(busiest.hour)}:00`;
  return (
    <header className="today">
      <div className="today__title">
        <h1>Today</h1>
        <p>{countLabel(saves)}</p>
      </div>
      <div className="today__bars" role="img" aria-label={label}>
        {buckets.map((b) => (
          <div key={b.hour} className={`today__hour${b.hour === current ? " is-now" : ""}`}>
            <span className="today__bar" style={{ height: `${b.count === 0 ? 0 : 15 + (85 * b.count) / most}%` }} />
            <span className="today__label">{b.hour % 3 === 0 ? pad(b.hour) : ""}</span>
          </div>
        ))}
      </div>
    </header>
  );
}
