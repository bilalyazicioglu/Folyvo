import type { CopyDto } from "../generated/CopyDto";
import type { ItemSummaryDto } from "../generated/ItemSummaryDto";
import type { SaveDto } from "../generated/SaveDto";
import { copyLabel } from "../copies";
import { fileKind } from "../files";
import { useCopies } from "../queries";
import { clock, middleTruncate, whenLabel } from "../time";

const NAME_MAX = 30;

interface Props {
  item: ItemSummaryDto;
  /** This file's saves and copies today, oldest first. */
  saves: SaveDto[];
  copies: CopyDto[];
  now: number;
  /** Start of the Today strip's first hour; the open row's line starts there. */
  start: number;
  open: boolean;
  onToggle: () => void;
  onOpenHistory: () => void;
}

function subLine(item: ItemSummaryDto, saves: SaveDto[], newestCopy: CopyDto | undefined, now: number): string {
  const kept =
    saves.length === 0
      ? `Last saved ${whenLabel(item.latestAt, now)}`
      : `${saves.length} ${saves.length === 1 ? "save" : "saves"} today`;
  return newestCopy ? `${kept}, ${copyLabel(newestCopy)} ${clock(newestCopy.at)}` : kept;
}

/** A recent file; clicked, it grows in place to show today's saves on a line. */
export function FileRow({ item, saves, copies, now, start, open, onToggle, onOpenHistory }: Props) {
  const newestCopy = copies.at(-1);
  const left = useCopies(open ? item.itemId : undefined);
  const copied = new Set((left.data ?? copies).map((c) => c.versionId));
  const span = Math.max(1, now - start);
  const regionId = `today-${item.itemId}`;
  return (
    <li className={`file${open ? " is-open" : ""}`}>
      <button type="button" className="file__head" aria-label={item.name} aria-expanded={open} aria-controls={regionId} onClick={onToggle}>
        <span className={`tile tile--${fileKind(item.name)}`} aria-hidden="true" />
        <span className="file__text">
          <span className="file__name" title={item.name}>
            {middleTruncate(item.name, NAME_MAX)}
          </span>
          <span className="file__sub">{subLine(item, saves, newestCopy, now)}</span>
        </span>
        <span className="file__when">{saves.length > 0 ? clock(item.latestAt) : ""}</span>
      </button>
      {open && (
        <section id={regionId} className="file__day" aria-label={`${item.name} today`}>
          <ol className="line">
            {saves.map((s) => (
              <li
                key={s.versionId}
                className="line__dot"
                data-copied={copied.has(s.versionId)}
                style={{ left: `${Math.min(100, Math.max(0, ((s.at - start) / span) * 100))}%` }}
                title={clock(s.at)}
              />
            ))}
          </ol>
          <div className="line__labels">
            <span>{String(new Date(start).getHours()).padStart(2, "0")}:00</span>
            {newestCopy && <span className="line__copy">{`${copyLabel(newestCopy)} ${clock(newestCopy.at)}`}</span>}
            <span>now</span>
          </div>
          <button type="button" className="btn btn--primary" onClick={onOpenHistory}>
            Open History
          </button>
        </section>
      )}
    </li>
  );
}
