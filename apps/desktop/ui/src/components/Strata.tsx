import type { VersionDto } from "../generated/VersionDto";
import { ago, bandHeight, bandOf, noteOf } from "../strata";

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
        <li key={v.id} className={`band band--${bandOf(v, i)}`} style={{ height: bandHeight(v.size) }}>
          <span className="band__seq">v{v.seq}</span>
          <span className="band__note">{noteOf(v)}</span>
          {v.isShareReady && (
            <span className="band__chip">
              <span aria-hidden="true">✓</span> Ready to share
            </span>
          )}
          <span className="band__time">{ago(v.createdAt, now)}</span>
        </li>
      ))}
    </ol>
  );
}
