import { useEffect, useRef, useState } from "react";
import { api, AppError } from "../api";
import { copyLabel } from "../copies";
import { DocIcon } from "../components/DocIcon";
import { ErrorNotice } from "../components/ErrorNotice";
import { fileKind } from "../files";
import { useAction, useCopies, useHistory, useItem, useNow, usePreview } from "../queries";
import { folderName, sizeLabel } from "../files";
import { needsSeconds, whenLabel } from "../time";
import { Stage } from "./Stage";
import { TimeList } from "./TimeList";

/** One file's saved times: a picture of each, and Open, Save a Copy and Restore. */
export function Versions({ itemId }: { itemId: string }) {
  const now = useNow();
  const file = useItem(itemId);
  const history = useHistory(itemId);
  const copies = useCopies(itemId);
  const act = useAction();
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);
  const cancelRef = useRef<HTMLButtonElement>(null);
  const restoreRef = useRef<HTMLButtonElement>(null);
  const asked = useRef(false);
  const [saved, setSaved] = useState<{ folder: string; path: string } | null>(null);

  const summary = file.data;
  const name = summary?.name ?? "";
  const versions = history.data ?? [];
  const selected = versions.find((v) => v.id === selectedId) ?? versions[0];
  const preview = usePreview(itemId, selected?.id);

  useEffect(() => {
    if (!name) return;
    document.title = name;
    void api.setWindowTitle(name).catch(() => {});
  }, [name]);

  // The question takes focus; dismissing it hands focus back to the button that asked it.
  useEffect(() => {
    if (confirming) cancelRef.current?.focus();
    else if (asked.current) restoreRef.current?.focus();
    asked.current = confirming;
  }, [confirming]);

  const failure = history.error ?? file.error;
  const loadError = failure instanceof AppError ? failure.info : null;
  if (!selected) {
    return (
      <main className="versions" aria-label="Saved versions" aria-busy={history.isPending}>
        {loadError && (
          <ErrorNotice error={loadError} onDismiss={() => void Promise.all([history.refetch(), file.refetch()])} />
        )}
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
  const openLabel = newest
    ? app
      ? `Open in ${app}`
      : "Open"
    : preview.data?.revealsCopy
      ? "Show a Copy in Finder"
      : app
        ? `Open a Copy in ${app}`
        : "Open a Copy";

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
      <section className="versions__main">
        <header className="versions__head" data-tauri-drag-region>
          <DocIcon name={name} />
          <div className="versions__title" data-tauri-drag-region>
            <h1>{name}</h1>
            <p>{left.length > 0 ? `${when}, ${copyLabel(left[left.length - 1])}` : when}</p>
          </div>
          <button
            type="button"
            className="btn btn--open"
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

        <Stage
          name={name}
          kind={kind}
          when={when}
          meta={`${when}, ${sizeLabel(selected.size)}${newest ? ". This is the file as it is now." : ""}`}
          preview={preview.data}
          failed={preview.isError}
          older={versions.length - 1 - index}
          newer={index}
          onOlder={() => select(versions[index + 1].id)}
          onNewer={() => select(versions[index - 1].id)}
        />

        {confirming && (
          <div role="group" aria-label="Confirm restore" className="confirm">
            <p>Restore {when}? Folyvo saves the current file first, so nothing is lost.</p>
            <div className="confirm__actions">
              <button type="button" className="btn btn--primary" disabled={act.busy} onClick={() => void restore()}>
                Restore
              </button>
              <button type="button" className="btn" ref={cancelRef} onClick={() => setConfirming(false)}>
                Cancel
              </button>
            </div>
          </div>
        )}

        <footer className="versions__bar">
          <div className="versions__pill">
            <button type="button" className="btn" onClick={() => void api.closeWindow()}>
              Done
            </button>
            <button type="button" className="btn" disabled={act.busy} onClick={() => void saveCopy()}>
              Save a Copy…
            </button>
            {!newest && !confirming && (
              <button
                type="button"
                className="btn btn--primary"
                ref={restoreRef}
                disabled={act.busy}
                onClick={() => setConfirming(true)}
              >
                Restore
              </button>
            )}
          </div>
        </footer>
      </section>
      <aside className="versions__side" aria-label="Saved times">
        <TimeList versions={versions} copies={copies.data ?? []} selected={selected.id} now={now} onSelect={select} />
      </aside>
    </main>
  );
}
