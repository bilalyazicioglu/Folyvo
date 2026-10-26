import { useEffect, useRef, useState } from "react";
import { api, AppError, INTERNAL_ERROR } from "../api";
import { NoFolders, WaitingForSave } from "../components/EmptyState";
import { ErrorNotice } from "../components/ErrorNotice";
import { FolderList } from "../components/FolderList";
import { useAction, useActivity, useFolders, useNow, useRecent } from "../queries";
import { folderName } from "../strata";
import { dayBounds, hourBuckets } from "../time";
import { DropResult, type Dropped } from "./DropResult";
import { FileRow } from "./FileRow";
import { TodayStrip } from "./TodayStrip";

const ROWS = 8;

function keeping(folders: { path: string }[]): string {
  return folders.length === 1
    ? `Keeping every save in ${folderName(folders[0].path)}`
    : `Keeping every save in ${folders.length} folders`;
}

/** Tracked file → its history; anything else → which saved version it is, if any. */
async function answerDrop(path: string): Promise<Dropped | null> {
  try {
    const item = await api.itemAt(path);
    if (item) {
      await api.openHistory(item);
      return null;
    }
    return { kind: "matches", matches: await api.which(path) };
  } catch (e) {
    const error = e instanceof AppError ? e.info : INTERNAL_ERROR;
    return error.code === "not_a_file" ? { kind: "folder" } : { kind: "error", error };
  }
}

export function Popover() {
  const now = useNow();
  const { from, to } = dayBounds(now);
  const folders = useFolders();
  const recent = useRecent();
  const activity = useActivity(from, to);
  const act = useAction();
  const root = useRef<HTMLElement>(null);
  const [openId, setOpenId] = useState<string | null>(null);
  const [dragging, setDragging] = useState(false);
  const [dropped, setDropped] = useState<Dropped | null>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") void api.hidePopover();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  useEffect(() => {
    const stop = api.onDrop((e) => {
      if (e.type !== "drop") return setDragging(e.type === "over");
      setDragging(false);
      if (e.paths[0]) void answerDrop(e.paths[0]).then(setDropped);
    });
    return () => void stop.then((f) => f());
  }, []);

  useEffect(() => {
    const el = root.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(() => void api.fitPopover(el.getBoundingClientRect().height));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const addFolder = async () => {
    const picked = await api.chooseFolder("Choose a folder to watch");
    if (picked) await act.run(() => api.addFolder(picked));
  };
  const removeFolder = (path: string) => void act.run(() => api.removeFolder(path));

  if (folders.isPending || recent.isPending) return <main className="popover" aria-busy="true" aria-label="Folyvo" />;

  const folderList = folders.data ?? [];
  const items = recent.data ?? [];
  const saves = [...(activity.data?.saves ?? [])].sort((a, b) => a.at - b.at);
  const copies = [...(activity.data?.copies ?? [])].sort((a, b) => a.at - b.at);
  const firstHour = hourBuckets(saves, now)[0].hour;
  const start = new Date(from).setHours(firstHour);

  return (
    <main className="popover" aria-label="Folyvo" ref={root}>
      {folderList.length > 0 && items.length > 0 && <TodayStrip saves={saves} now={now} />}

      {act.error && <ErrorNotice error={act.error} onDismiss={act.clearError} />}
      {dropped && (
        <DropResult
          result={dropped}
          now={now}
          onOpenHistory={(item) => void api.openHistory(item)}
          onDismiss={() => setDropped(null)}
        />
      )}

      <div className="popover__body">
        {folderList.length === 0 ? (
          <NoFolders onChoose={() => void addFolder()} busy={act.busy} />
        ) : items.length === 0 ? (
          <WaitingForSave folder={folderName(folderList[0].path)} />
        ) : (
          <ul className="rows">
            {items.slice(0, ROWS).map((it) => (
              <FileRow
                key={it.itemId}
                item={it}
                saves={saves.filter((s) => s.itemId === it.itemId)}
                copies={copies.filter((c) => c.itemId === it.itemId)}
                now={now}
                start={start}
                open={openId === it.itemId}
                onToggle={() => setOpenId(openId === it.itemId ? null : it.itemId)}
                onOpenHistory={() => void api.openHistory(it.itemId)}
              />
            ))}
          </ul>
        )}
      </div>

      {folderList.length > 0 && (
        <footer className="popover__foot">
          <div className="popover__keeping">
            <p>{keeping(folderList)}</p>
            <button type="button" className="btn btn--link" disabled={act.busy} onClick={() => void addFolder()}>
              Add Folder…
            </button>
          </div>
          <FolderList folders={folderList} onRemove={removeFolder} busy={act.busy} />
        </footer>
      )}

      {dragging && (
        <div className="drop-overlay" aria-live="polite">
          <p>Drop a file to see which saved version it is</p>
        </div>
      )}
    </main>
  );
}
