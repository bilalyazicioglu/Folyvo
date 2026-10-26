import type { MatchDto } from "../generated/MatchDto";
import type { UserError } from "../generated/UserError";
import { copyLabel } from "../copies";
import { whenLabel } from "../time";

export type Dropped = { kind: "matches"; matches: MatchDto[] } | { kind: "folder" } | { kind: "error"; error: UserError };

interface Props {
  result: Dropped;
  now: number;
  onOpenHistory: (item: string) => void;
  onDismiss: () => void;
}

/** The answer to "which one is this?" for a file Folyvo is not watching. */
export function DropResult({ result, now, onOpenHistory, onDismiss }: Props) {
  return (
    <div role="status" className="notice drop-result">
      {result.kind === "folder" ? (
        <p>Drop a file, not a folder.</p>
      ) : result.kind === "error" ? (
        <p>
          <strong>{result.error.title}</strong> {result.error.hint}
        </p>
      ) : result.matches.length === 0 ? (
        <p>No saved version has exactly this content. It may have been opened and saved again since it left.</p>
      ) : (
        <ul>
          {result.matches.map((m) => (
            <li key={m.version.id}>
              <p>{`${m.name}, ${whenLabel(m.version.createdAt, now)} (${m.version.code})`}</p>
              {m.copies.map((c) => (
                <p key={`${c.kind}-${c.at}`} className="drop-result__copy">{`${copyLabel(c)} ${whenLabel(c.at, now)}`}</p>
              ))}
              <button type="button" className="btn btn--primary" onClick={() => onOpenHistory(m.itemId)}>
                Open History
              </button>
            </li>
          ))}
        </ul>
      )}
      <button type="button" className="btn btn--link" onClick={onDismiss}>
        Dismiss
      </button>
    </div>
  );
}
