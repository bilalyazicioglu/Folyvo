import { useEffect, useState } from "react";
import { api, AppError } from "../api";
import { copyLabel } from "../copies";
import { ErrorNotice } from "../components/ErrorNotice";
import { fileKind } from "../files";
import { useAction, useCopies, useHistory, useNow, usePreview, useRecent } from "../queries";
import { folderName, sizeLabel } from "../strata";
import { needsSeconds, whenLabel } from "../time";
import { Stage } from "./Stage";
import { TimeList } from "./TimeList";

/** One file's saved times: a picture of each, and Open, Save a Copy and Restore. */
export function Versions({ itemId }: { itemId: string }) {
  const now = useNow();
  const recent = useRecent();
  const history = useHistory(itemId);
  const copies = useCopies(itemId);
  const act = useAction();
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [saved, setSaved] = useState<{ folder: string; path: string } | null>(null);

  const summary = recent.data?.find((i) => i.itemId === itemId);
  const name = summary?.name ?? "";
  const versions = history.data ?? [];
  const selected = versions.find((v) => v.id === selectedId) ?? versions[0];
  const preview = usePreview(itemId, selected?.id);

  useEffect(() => {
    if (!name) return;
    document.title = name;
    void api.setWindowTitle(name).catch(() => {});
  }, [name]);

  const loadError = history.error instanceof AppError ? history.error.info : null;
  if (!selected) {
    return (
      <main className="versions" aria-label="Saved versions" aria-busy={history.isPending}>
        {loadError && <ErrorNotice error={loadError} onDismiss={() => void history.refetch()} />}
      </main>
    );
  }

  const index = versions.indexOf(selected);
  const newest = index === 0 && !summary?.missing;
  const seconds = needsSeconds(versions.map((v) => v.createdAt));
  const when = whenLabel(selected.createdAt, now, seconds.has(selected.createdAt));
  const left = (copies.data ?? []).filter((c) => c.versionId === selected.id);
  const kind = fileKind(name);
  const app = preview.data?.appName;
  const openLabel = newest ? (app ? `Open in ${app}` : "Open") : app ? `Open a Copy in ${app}` : "Open a Copy";

  const select = (id: string) => {
    setSelectedId(id);
    setConfirming(false);
  };

  const saveCopy = async () => {
    const dir = await api.chooseFolder("Choose where to save the copy");
    if (!dir) return;
    const r = await act.run(() => api.exportVersion(itemId, selected.id, dir));
    if (r.ok) setSaved({ folder: folderName(dir), path: r.value.destination });
  };

  const restore = async () => {
    setConfirming(false);
    const r = await act.run(() => api.restore(itemId, selected.id));
    if (!r.ok) return;
    setSelectedId(null);
    void api.notify(`Back to ${when}`, `${name}. Today's version is still in its history.`);
  };

  return (
    <main className="versions" aria-label="Saved versions">
      <header className="versions__head">
        <span className={`tile tile--${kind}`} aria-hidden="true" />
        <div className="versions__title">
          <h1>{name}</h1>
          <p>{left.length > 0 ? `${when}, ${copyLabel(left[left.length - 1])}` : when}</p>
        </div>
        <button
          type="button"
          className="btn"
          disabled={act.busy}
          onClick={() => void act.run(() => api.openVersion(itemId, selected.id, newest))}
        >
          {openLabel}
        </button>
      </header>

      {act.error && <ErrorNotice error={act.error} onDismiss={act.clearError} />}
      {saved && !act.error && (
        <div role="status" className="notice">
          <div>Saved a copy to {saved.folder}</div>
          <button type="button" className="btn btn--link" onClick={() => void api.reveal(saved.path)}>
            Show in Finder
          </button>
          <button type="button" className="btn btn--link" onClick={() => setSaved(null)}>
            Dismiss
          </button>
        </div>
      )}

      <div className="versions__body">
        <Stage
          name={name}
          kind={kind}
          when={when}
          meta={`${when}, ${sizeLabel(selected.size)}${newest ? ". This is the file as it is now." : ""}`}
          preview={preview.data}
          older={versions.length - 1 - index}
        />
        <TimeList versions={versions} copies={copies.data ?? []} selected={selected.id} now={now} onSelect={select} />
      </div>

      {confirming && (
        <div role="group" aria-label="Confirm restore" className="confirm">
          <p>Restore {when}? Folyvo saves the current file first, so nothing is lost.</p>
          <div className="confirm__actions">
            <button type="button" className="btn btn--primary" disabled={act.busy} onClick={() => void restore()}>
              Restore
            </button>
            <button type="button" className="btn" onClick={() => setConfirming(false)}>
              Cancel
            </button>
          </div>
        </div>
      )}

      <footer className="versions__bar">
        <button type="button" className="btn" onClick={() => void api.closeWindow()}>
          Done
        </button>
        <span className="versions__spacer" />
        <button type="button" className="btn" disabled={act.busy} onClick={() => void saveCopy()}>
          Save a Copy…
        </button>
        {!newest && !confirming && (
          <button type="button" className="btn btn--primary" disabled={act.busy} onClick={() => setConfirming(true)}>
            Restore
          </button>
        )}
      </footer>
    </main>
  );
}
