import type { VersionDto } from "../generated/VersionDto";
import { ago, bandHeight, bandOf, noteOf } from "../strata";

export function Band({ version, index, now }: { version: VersionDto; index: number; now: number }) {
  return (
    <div className={`band band--${bandOf(version, index)}`} style={{ height: bandHeight(version.size) }}>
      <span className="band__seq">v{version.seq}</span>
      <span className="band__note">{noteOf(version)}</span>
      <span className="band__time">{ago(version.createdAt, now)}</span>
    </div>
  );
}
