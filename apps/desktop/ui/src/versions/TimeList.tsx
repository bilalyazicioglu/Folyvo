import { useRef, type KeyboardEvent } from "react";
import { copyLabel } from "../copies";
import type { CopyDto } from "../generated/CopyDto";
import type { VersionDto } from "../generated/VersionDto";
import { clock, needsSeconds, whenLabel } from "../time";

interface Props {
  versions: VersionDto[];
  copies: CopyDto[];
  selected: string;
  now: number;
  onSelect: (id: string) => void;
}

/** "Today", "Yesterday", a weekday or a date: the day part of a when label. */
const dayOf = (at: number, now: number) => whenLabel(at, now).replace(/ \S+$/, "");

/** Every saved time, newest first, grouped by day; arrow keys move the selection. */
export function TimeList({ versions, copies, selected, now, onSelect }: Props) {
  const refs = useRef(new Map<string, HTMLLIElement>());
  const seconds = needsSeconds(versions.map((v) => v.createdAt));
  const groups: { day: string; versions: VersionDto[] }[] = [];
  for (const v of versions) {
    const day = dayOf(v.createdAt, now);
    if (groups.at(-1)?.day === day) groups.at(-1)!.versions.push(v);
    else groups.push({ day, versions: [v] });
  }

  const move = (e: KeyboardEvent) => {
    const step = e.key === "ArrowDown" ? 1 : e.key === "ArrowUp" ? -1 : 0;
    if (!step) return;
    e.preventDefault();
    const i = versions.findIndex((v) => v.id === selected);
    const next = versions[Math.min(versions.length - 1, Math.max(0, i + step))];
    onSelect(next.id);
    refs.current.get(next.id)?.focus();
  };

  return (
    <div className="times" role="listbox" aria-label="Saved times" onKeyDown={move}>
      {groups.map((g) => (
        <ul key={g.day} role="group" aria-label={g.day}>
          <li className="times__day" role="presentation" aria-hidden="true">
            {g.day}
          </li>
          {g.versions.map((v) => {
            const left = copies.filter((c) => c.versionId === v.id);
            return (
              <li
                key={v.id}
                ref={(el) => void (el ? refs.current.set(v.id, el) : refs.current.delete(v.id))}
                role="option"
                aria-selected={v.id === selected}
                tabIndex={v.id === selected ? 0 : -1}
                className="times__row"
                data-copied={left.length > 0}
                onClick={() => onSelect(v.id)}
              >
                <span className="times__rail" aria-hidden="true" />
                <span className="times__time">{clock(v.createdAt, seconds.has(v.createdAt))}</span>
                {v.note && <em className="times__note">{v.note}</em>}
                {left.map((c) => (
                  <span key={`${c.kind}-${c.at}`} className="chip">
                    {copyLabel(c)}
                  </span>
                ))}
              </li>
            );
          })}
        </ul>
      ))}
    </div>
  );
}
