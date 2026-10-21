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

export function WaitingForSave({ folder }: { folder: string }) {
  return (
    <section className="empty">
      <h2>Waiting for the first save</h2>
      <p>Save a file in {folder} and it shows up here.</p>
    </section>
  );
}
