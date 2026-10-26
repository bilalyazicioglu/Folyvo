import { useEffect, useState } from "react";
import { api } from "../api";
import { NoFolders, WaitingForSave } from "../components/EmptyState";
import { ErrorNotice } from "../components/ErrorNotice";
import { FolderList } from "../components/FolderList";
import { Strata } from "../components/Strata";
import { useAction, useFolders, useHistory, useNow, useRecent, useStats } from "../queries";
import { ago, exportLabel, folderName, storedLabel } from "../strata";

interface Notice {
  text: string;
  reveal?: string;
}

function watching(folders: { path: string }[]): string {
  return folders.length === 1 ? `Watching ${folderName(folders[0].path)}` : `Watching ${folders.length} folders`;
}

export function Popover() {
  const now = useNow();
  const folders = useFolders();
  const recent = useRecent();
  const stats = useStats();
  const current = recent.data?.[0];
  const history = useHistory(current?.itemId);
  const act = useAction();
  const [notice, setNotice] = useState<Notice | null>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") void api.hidePopover();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const addFolder = async () => {
    const picked = await api.chooseFolder("Choose a folder to watch");
    if (picked) await act.run(() => api.addFolder(picked));
  };
  const removeFolder = (path: string) => void act.run(() => api.removeFolder(path));

  if (folders.isPending || recent.isPending) return <main className="popover" aria-busy="true" aria-label="Folyvo" />;

  const folderList = folders.data ?? [];
  const items = recent.data ?? [];
  const versions = history.data ?? [];
  const latest = versions[0];

  const exportLatest = async () => {
    if (!current || !latest) return;
    const dir = await api.chooseFolder("Choose where to put the copy");
    if (!dir) return;
    const r = await act.run(() => api.exportVersion(current.itemId, latest.id, dir));
    if (r.ok) setNotice({ text: `Exported v${latest.seq}`, reveal: r.value.destination });
  };

  return (
    <main className="popover" aria-label="Folyvo">
      <header className="popover__head">
        <h1>Folyvo</h1>
        {folderList.length > 0 && <p>{watching(folderList)}</p>}
      </header>

      {act.error && <ErrorNotice error={act.error} onDismiss={act.clearError} />}
      {notice && !act.error && (
        <div role="status" className="notice">
          <div>{notice.text}</div>
          {notice.reveal && (
            <button type="button" className="btn btn--link" onClick={() => void api.reveal(notice.reveal!)}>
              Show in Finder
            </button>
          )}
          <button type="button" className="btn btn--link" onClick={() => setNotice(null)}>
            Dismiss
          </button>
        </div>
      )}

      <div className="popover__body">
        {folderList.length === 0 ? (
          <NoFolders onChoose={() => void addFolder()} busy={act.busy} />
        ) : !current ? (
          <WaitingForSave folder={folderName(folderList[0].path)} />
        ) : (
          <>
            <section className="card" aria-label={current.name}>
              <div className="card__title">
                <h2 title={current.path ?? current.name}>{current.name}</h2>
                <span>{ago(current.latestAt, now)}</span>
              </div>
              {current.missing && (
                <p className="card__status">This file was removed from its folder. Its versions are still saved.</p>
              )}
              <Strata versions={versions} now={now} limit={4} />
              {!current.missing && latest && (
                <div className="card__actions">
                  <button type="button" className="btn btn--primary" disabled={act.busy} onClick={() => void exportLatest()}>
                    {exportLabel(latest.seq)}
                  </button>
                </div>
              )}
              <button type="button" className="btn btn--link" onClick={() => void api.openHistory(current.itemId)}>
                All versions
              </button>
            </section>

            {items.length > 1 && (
              <>
                <h2 className="section-title">Recent</h2>
                <ul className="recent">
                  {items.slice(1, 6).map((it) => (
                    <li key={it.itemId}>
                      <button type="button" onClick={() => void api.openHistory(it.itemId)}>
                        <span className="name">{it.name}</span>
                        <span className="when">{ago(it.latestAt, now)}</span>
                      </button>
                    </li>
                  ))}
                </ul>
              </>
            )}
          </>
        )}
        {folderList.length > 0 && (
          <FolderList folders={folderList} onAdd={() => void addFolder()} onRemove={removeFolder} busy={act.busy} />
        )}
      </div>

      {stats.data && <footer className="popover__foot">{storedLabel(stats.data)}</footer>}
    </main>
  );
}
