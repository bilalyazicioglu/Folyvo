import type { FolderDto } from "../generated/FolderDto";
import type { ReadingState } from "../generated/ReadingState";

export function NoFolders({ onChoose, busy }: { onChoose: () => void; busy: boolean }) {
  return (
    <section className="empty">
      <h2>Choose a folder to watch</h2>
      <p>Folyvo keeps every saved version of the files in it. Everything stays on this Mac.</p>
      <button type="button" className="btn btn--primary" onClick={onChoose} disabled={busy}>
        Choose folder
      </button>
    </section>
  );
}

function timeLeft(seconds: number): string {
  if (seconds < 60) return "Less than a minute left";
  const minutes = Math.round(seconds / 60);
  return minutes === 1 ? "About a minute left" : `About ${minutes} minutes left`;
}

/** `beside` is the smaller form that sits above the files already kept. */
export function ReadingFolders({
  folders,
  onPause,
  onResume,
  onStop,
  beside = false,
}: {
  folders: FolderDto[];
  onPause: (path: string) => void;
  onResume: (path: string) => void;
  onStop: (path: string) => void;
  beside?: boolean;
}) {
  const inState = (state: ReadingState) => folders.filter((f) => f.reading?.state === state);
  const going = inState("reading");
  const paused = inState("paused");
  const reading = folders.flatMap((f) => (f.reading ? [f.reading] : []));
  const allPaused = paused.length === reading.length;
  const done = reading.reduce((sum, r) => sum + r.done, 0);
  const total = reading.reduce((sum, r) => sum + r.total, 0);
  const lefts = reading.map((r) => r.secondsLeft);
  // Every folder must have an estimate, or the slowest one could be the one without.
  const left = lefts.every((s) => s !== null) ? Math.max(...(lefts as number[])) : null;
  return (
    <section className={beside ? "empty empty--reading empty--beside" : "empty empty--reading"}>
      <h2 id="reading-files">{allPaused ? "Reading paused" : "Reading your files"}</h2>
      {total > 0 ? (
        <progress className="reading-bar" aria-labelledby="reading-files" value={done} max={total} />
      ) : (
        !allPaused && <progress className="reading-bar" aria-labelledby="reading-files" />
      )}
      <div className="reading-count">
        {total > 0 && <span>{`${done.toLocaleString("en-US")} of ${total.toLocaleString("en-US")} files`}</span>}
        {left !== null && <span>{timeLeft(left)}</span>}
      </div>
      <p>
        {allPaused
          ? "What Folyvo read is kept for a day. Add the folder again or resume before then to go on."
          : beside
            ? "New files show up here when Folyvo has finished."
            : "You can keep working. Your files show up here when Folyvo has finished."}
      </p>
      {going.length > 0 ? (
        <button type="button" className="btn btn--link" onClick={() => going.forEach((f) => onPause(f.path))}>
          Pause Reading
        </button>
      ) : allPaused ? (
        <div className="reading-actions">
          <button type="button" className="btn btn--link" onClick={() => paused.forEach((f) => onResume(f.path))}>
            Resume Reading
          </button>
          <button type="button" className="btn btn--link" onClick={() => paused.forEach((f) => onStop(f.path))}>
            Stop Reading
          </button>
        </div>
      ) : (
        <p>{inState("pausing").length > 0 ? "Pausing…" : "Stopping…"}</p>
      )}
    </section>
  );
}

export function WaitingForSave({ folder }: { folder: string }) {
  return (
    <section className="empty">
      <h2>Waiting for the first save</h2>
      <p>Save a file in {folder} and it shows up here.</p>
    </section>
  );
}
