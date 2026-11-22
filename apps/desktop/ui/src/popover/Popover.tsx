import { useCallback, useEffect, useRef, useState } from "react";
import { api, AppError, INTERNAL_ERROR } from "../api";
import { NoFolders, ReadingFolders, WaitingForSave } from "../components/EmptyState";
import { ErrorNotice } from "../components/ErrorNotice";
import { FolderList } from "../components/FolderList";
import { useAction, useActivity, useFolders, useLettingGo, useMissed, useNow, useRecent, useSpace, useUpdates } from "../queries";
import { folderName, sizeLabel } from "../files";
import type { FolderDto } from "../generated/FolderDto";
import { dayBounds, hourBuckets } from "../time";
import { DropResult, type Dropped } from "./DropResult";
import { FileRow } from "./FileRow";
import { LetGoQuestion } from "./LetGoQuestion";
import { MissedNotice } from "./MissedNotice";
import { TodayStrip } from "./TodayStrip";
import { UpdateNotice } from "./UpdateNotice";

const ROWS = 8;

/** `lettingGo`: older saves are let go, so not every save is kept. */
function keeping(folders: FolderDto[], lettingGo: boolean): string {
  const reading = folders.filter((f) => f.reading);
  if (reading.length > 0 && reading.every((f) => f.reading?.state === "paused"))
    return reading.length === 1 ? `Paused reading ${folderName(reading[0].path)}` : `Paused reading ${reading.length} folders`;
  if (reading.length === 1) return `Reading ${folderName(reading[0].path)}…`;
  if (reading.length > 1) return `Reading ${reading.length} folders…`;
  const what = lettingGo ? "saves" : "every save";
  return folders.length === 1
    ? `Keeping ${what} in ${folderName(folders[0].path)}`
    : `Keeping ${what} in ${folders.length} folders`;
}

/** A failed lookup: a folder is refused plainly, anything else shows its error. */
function failed(e: unknown): Dropped {
  const error = e instanceof AppError ? e.info : INTERNAL_ERROR;
  return error.code === "not_a_file" ? { kind: "folder" } : { kind: "error", error };
}

/** Which saved version a file is, if any. */
async function answerWhich(path: string): Promise<Dropped> {
  try {
    return { kind: "matches", matches: await api.which(path) };
  } catch (e) {
    return failed(e);
  }
}

/** Tracked file → its history; anything else → which saved version it is, if any. */
async function answerDrop(path: string): Promise<Dropped | null> {
  try {
    const item = await api.itemAt(path);
    if (item) {
      await api.openHistory(item);
      return null;
    }
  } catch (e) {
    return failed(e);
  }
  return answerWhich(path);
}

export function Popover() {
  const now = useNow();
  const { from, to } = dayBounds(now);
  const folders = useFolders();
  const recent = useRecent();
  const activity = useActivity(from, to);
  const missed = useMissed();
  const letting = useLettingGo();
  const updates = useUpdates();
  const space = useSpace(0);
  const act = useAction();
  const observer = useRef<ResizeObserver | null>(null);
  // A callback ref: the loading shell and the loaded popover are different elements, and both must be watched.
  const root = useCallback((el: HTMLElement | null) => {
    observer.current?.disconnect();
    observer.current = null;
    if (!el || typeof ResizeObserver === "undefined") return;
    observer.current = new ResizeObserver(() => void api.fitPopover(el.getBoundingClientRect().height));
    observer.current.observe(el);
  }, []);
  const [openId, setOpenId] = useState<string | null>(null);
  const [dragging, setDragging] = useState(false);
  const [dropped, setDropped] = useState<Dropped | null>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") void api.hidePopover();
    };
    // The window is larger than the glass, to leave room for its shadow. That clear space is outside.
    const onPress = (e: MouseEvent) => {
      if (e.target instanceof Element && !e.target.closest(".popover")) void api.hidePopover();
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("mousedown", onPress);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("mousedown", onPress);
    };
  }, []);

  useEffect(() => {
    const stop = api.onDrop((e) => {
      if (e.type !== "drop") return setDragging(e.type === "over");
      setDragging(false);
      if (e.paths[0]) void answerDrop(e.paths[0]).then(setDropped);
    });
    return () => void stop.then((f) => f());
  }, []);

  // "Which One Is This?…" from Finder or Services.
  useEffect(() => {
    const stop = api.onIntentWhich((path) => void answerWhich(path).then(setDropped));
    return () => void stop.then((f) => f());
  }, []);

  const addFolder = async () => {
    const picked = await api.chooseFolder("Choose a folder to watch");
    if (picked) await act.run(() => api.addFolder(picked));
  };
  const removeFolder = (path: string) => void act.run(() => api.removeFolder(path));
  const pauseFolder = (path: string) => void act.run(() => api.pauseFolder(path));
  const resumeFolder = (path: string) => void act.run(() => api.resumeFolder(path));
  const stopReading = (path: string) => void act.run(() => api.stopReading(path));

  if (folders.isPending || recent.isPending)
    return <main className="popover" aria-busy="true" aria-label="Folyvo" ref={root} />;

  const folderList = folders.data ?? [];
  // The list leaves out what a read is bringing in, so it holds only what the person already knows.
  const shown = folderList.filter((f) => f.reading && !f.reading.quiet);
  const reading = shown.length > 0;
  const items = recent.data ?? [];
  const saves = [...(activity.data?.saves ?? [])].sort((a, b) => a.at - b.at);
  const copies = [...(activity.data?.copies ?? [])].sort((a, b) => a.at - b.at);
  const firstHour = hourBuckets(saves, now)[0].hour;
  const start = new Date(from).setHours(firstHour);

  return (
    <main className="popover" aria-label="Folyvo" ref={root}>
      {!reading && folderList.length > 0 && items.length > 0 && <TodayStrip saves={saves} now={now} />}

      {act.error && <ErrorNotice error={act.error} onDismiss={act.clearError} />}
      {!!missed.data && (
        <MissedNotice count={missed.data} onDismiss={() => void act.run(() => api.dismissMissed())} />
      )}
      {letting.data?.rule === "unasked" && letting.data.waiting.saves > 0 && (
        <LetGoQuestion
          bytes={letting.data.waiting.bytes}
          busy={act.busy}
          onAnswer={(on) => void act.run(() => api.setLettingGo(on))}
        />
      )}
      {updates.data && (
        <UpdateNotice updates={updates.data} busy={act.busy} onInstall={() => void act.run(() => api.installUpdate())} />
      )}
      {dropped && (
        <DropResult
          result={dropped}
          now={now}
          onOpenHistory={(item) => void api.openHistory(item)}
          onDismiss={() => setDropped(null)}
        />
      )}

      {reading && items.length > 0 && <ReadingFolders folders={shown} onPause={pauseFolder} onResume={resumeFolder} onStop={stopReading} beside />}

      <div className="popover__body">
        {folderList.length === 0 ? (
          <NoFolders onChoose={() => void addFolder()} busy={act.busy} />
        ) : items.length === 0 ? (
          reading ? (
            <ReadingFolders folders={shown} onPause={pauseFolder} onResume={resumeFolder} onStop={stopReading} />
          ) : (
            <WaitingForSave folder={folderName(folderList[0].path)} />
          )
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

      <footer className="popover__foot">
        <div className="popover__actions">
          {folderList.length > 0 && items.length > 0 && (
            <button type="button" className="btn btn--link popover__all" onClick={() => void api.openAllFiles()}>
              All Files…
            </button>
          )}
          {folderList.length > 0 && space.data && (
            <button type="button" className="btn btn--link popover__space" onClick={() => void api.openSpace()}>
              Uses {sizeLabel(space.data.bytes)}
            </button>
          )}
          <button type="button" className="btn btn--link popover__quit" onClick={() => void api.quit()}>
            Quit Folyvo
          </button>
        </div>
        {folderList.length > 0 && (
          <div className="popover__keeping">
            <FolderList folders={folderList} onRemove={removeFolder} busy={act.busy} summary={keeping(folderList, letting.data?.rule === "on")} />
            <button type="button" className="btn btn--link" disabled={act.busy} onClick={() => void addFolder()}>
              Add Folder…
            </button>
          </div>
        )}
      </footer>

      {dragging && (
        <div className="drop-overlay" aria-live="polite">
          <p>Drop a file to see which saved version it is</p>
        </div>
      )}
    </main>
  );
}
