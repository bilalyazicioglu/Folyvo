import type { VersionDto } from "../generated/VersionDto";
import { ago, bandHeight, bandOf, noteOf } from "../strata";

export function Band({ version, index, now, justMarked }: { version: VersionDto; index: number; now: number; justMarked?: boolean }) {
  return (
    <div className={`band band--${bandOf(version, index)}${justMarked ? " band--just-marked" : ""}`} style={{ height: bandHeight(version.size) }}>
      <span className="band__seq">v{version.seq}</span>
      <span className="band__note">{noteOf(version)}</span>
      {version.isShareReady && (
        <span className="band__chip">
          <span aria-hidden="true">✓</span> Ready to share
        </span>
      )}
      <span className="band__time">{ago(version.createdAt, now)}</span>
    </div>
  );
}
