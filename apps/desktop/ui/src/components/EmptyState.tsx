import type { FolderDto } from "../generated/FolderDto";
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

export function ReadingFolders({ folders }: { folders: FolderDto[] }) {
  const reading = folders.flatMap((f) => (f.reading ? [f.reading] : []));
  const done = reading.reduce((sum, r) => sum + r.done, 0);
  const total = reading.reduce((sum, r) => sum + r.total, 0);
  const lefts = reading.map((r) => r.secondsLeft);
  // Every folder must have an estimate, or the slowest one could be the one without.
  const left = lefts.every((s) => s !== null) ? Math.max(...(lefts as number[])) : null;
  return (
    <section className="empty empty--reading">
      <h2 id="reading-files">Reading your files</h2>
      {total > 0 ? (
        <progress className="reading-bar" aria-labelledby="reading-files" value={done} max={total} />
      ) : (
        <progress className="reading-bar" aria-labelledby="reading-files" />
      )}
      <div className="reading-count">
        {total > 0 && <span>{`${done.toLocaleString("en-US")} of ${total.toLocaleString("en-US")} files`}</span>}
        {left !== null && <span>{timeLeft(left)}</span>}
      </div>
      <p>You can keep working. Your files show up here when Folyvo has finished.</p>
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
