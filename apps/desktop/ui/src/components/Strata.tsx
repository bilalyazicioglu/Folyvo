import type { VersionDto } from "../generated/VersionDto";
import { Band } from "./Band";

interface Props {
  versions: VersionDto[];
  now: number;
  limit?: number;
}

export function Strata({ versions, now, limit }: Props) {
  const shown = limit ? versions.slice(0, limit) : versions;
  return (
    <ol className="strata" aria-label="Versions, newest first">
      {shown.map((v, i) => (
        <li key={v.id} className="strata__item">
          <Band version={v} index={i} now={now} />
        </li>
      ))}
    </ol>
  );
}
