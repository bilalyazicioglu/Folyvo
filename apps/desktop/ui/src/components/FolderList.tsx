import type { FolderDto } from "../generated/FolderDto";
import type { ReadingState } from "../generated/ReadingState";
import { folderName } from "../files";

interface Props {
  folders: FolderDto[];
  /** Without it the list has no add button of its own. */
  onAdd?: () => void;
  onRemove: (path: string) => void;
  busy: boolean;
  defaultOpen?: boolean;
  /** The line that opens the list; defaults to the folder count. */
  summary?: string;
}

const READING_LABEL: Record<ReadingState, string> = {
  reading: "Reading…",
  pausing: "Pausing…",
  paused: "Paused",
  stopping: "Stopping…",
};

export function FolderList({ folders, onAdd, onRemove, busy, defaultOpen, summary }: Props) {
  return (
    <details className="folders" open={defaultOpen}>
      <summary>{summary ?? `Watched folders (${folders.length})`}</summary>
      <ul>
        {folders.map((f) => (
          <li key={f.path}>
            <div className="path" title={f.path}>
              {folderName(f.path)}
              {f.reading && <div className="reading">{READING_LABEL[f.reading.state]}</div>}
              {f.problem && (
                <div className="problem">
                  <strong>{f.problem.title}</strong>
                  <div>{f.problem.hint}</div>
                </div>
              )}
            </div>
            <button type="button" className="btn btn--link" disabled={busy || f.reading?.state === "stopping"} onClick={() => onRemove(f.path)} aria-label={`${f.reading ? "Stop and remove" : "Stop watching"} ${folderName(f.path)}`}>
              {f.reading ? "Stop and Remove" : "Stop watching"}
            </button>
          </li>
        ))}
      </ul>
      {onAdd && (
        <button type="button" className="btn" onClick={onAdd} disabled={busy}>
          Add folder
        </button>
      )}
    </details>
  );
}
