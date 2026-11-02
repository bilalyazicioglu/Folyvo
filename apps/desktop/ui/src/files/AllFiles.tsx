import { useEffect, useId, useMemo, useRef, useState } from "react";
import { groupFiles, matchesName, type FileEntry } from "../allFiles";
import { api } from "../api";
import { DocIcon } from "../components/DocIcon";
import { ErrorNotice } from "../components/ErrorNotice";
import { useAction, useAllFiles, useFolders, useNow } from "../queries";
import { middleTruncate, whenLabel } from "../time";

/** Long names lose their middle: similar names differ at the end. The full name is the row's tooltip. */
const NAME_MAX = 64;

function subLine({ item, subfolder }: FileEntry, now: number): string {
  const saved = `last saved ${whenLabel(item.latestAt, now)}`;
  const where = item.missing ? `Moved or deleted, ${saved}` : subfolder ? `${subfolder}, ${saved}` : saved;
  return where[0].toUpperCase() + where.slice(1);
}

/** Every file Folyvo keeps, found by typing part of its name. */
export function AllFiles() {
  const now = useNow();
  const files = useAllFiles();
  const folders = useFolders();
  const act = useAction();
  const id = useId();
  const [typed, setTyped] = useState("");
  const [active, setActive] = useState(0);
  const search = useRef<HTMLInputElement>(null);
  const activeRow = useRef<HTMLDivElement>(null);

  const groups = useMemo(
    () => groupFiles((files.data ?? []).filter((f) => matchesName(f.name, typed)), folders.data ?? []),
    [files.data, folders.data, typed],
  );
  const entries = groups.flatMap((g) => g.files);
  const current = Math.min(active, entries.length - 1);
  const named = groups.length > 1;

  useEffect(() => {
    search.current?.focus();
  }, []);
  useEffect(() => {
    activeRow.current?.scrollIntoView?.({ block: "nearest" });
  }, [current]);

  const open = (itemId: string) => void api.openHistory(itemId);
  const addFolder = async () => {
    const picked = await api.chooseFolder("Choose a folder to watch");
    if (picked) await act.run(() => api.addFolder(picked));
  };

  const clearSearch = () => {
    setTyped("");
    setActive(0);
    search.current?.focus();
  };

  const onKey = (e: React.KeyboardEvent) => {
    // This Enter or arrow belongs to the input method still composing a word.
    if (e.nativeEvent.isComposing) return;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const step = e.key === "ArrowDown" ? 1 : -1;
      setActive(Math.max(0, Math.min(entries.length - 1, current + step)));
    } else if (e.key === "Enter" && entries[current]) {
      open(entries[current].item.itemId);
    } else if (e.key === "Escape") {
      if (typed) {
        clearSearch();
      } else {
        void api.closeWindow();
      }
    }
  };

  const loaded = !files.isPending && !folders.isPending;
  const nothingKept = loaded && (files.data ?? []).length === 0;
  let index = -1;
  const row = (entry: FileEntry) => {
    index += 1;
    const at = index;
    const { item } = entry;
    return (
      <div
        key={item.itemId}
        id={`${id}-${at}`}
        ref={at === current ? activeRow : undefined}
        role="option"
        aria-selected={at === current}
        className={`all__row${at === current ? " is-active" : ""}${item.missing ? " is-missing" : ""}`}
        onClick={() => open(item.itemId)}
      >
        <DocIcon name={item.name} />
        <span className="file__text">
          <span className="file__name" title={item.name} data-testid="name">
            {middleTruncate(item.name, NAME_MAX)}
          </span>
          <span className="file__sub">{subLine(entry, now)}</span>
        </span>
      </div>
    );
  };

  return (
    <main
      className="all"
      aria-label="All Files"
      aria-busy={!loaded}
      // A click anywhere but a button leaves the cursor in the search, so the keyboard keeps working.
      onClick={(e) => {
        if (!(e.target as Element).closest("button")) search.current?.focus();
      }}
    >
      <header className="all__head" data-tauri-drag-region>
        <input
          ref={search}
          className="all__search"
          type="search"
          role="combobox"
          aria-label="Find a file"
          aria-controls={`${id}-list`}
          aria-expanded={entries.length > 0}
          aria-activedescendant={entries.length > 0 ? `${id}-${current}` : undefined}
          placeholder="Find a file"
          autoComplete="off"
          spellCheck={false}
          value={typed}
          onChange={(e) => {
            setTyped(e.target.value);
            setActive(0);
          }}
          onKeyDown={onKey}
        />
      </header>

      {act.error && <ErrorNotice error={act.error} onDismiss={act.clearError} />}
      <p role="status" className="sr-only">
        {loaded && `${entries.length} ${entries.length === 1 ? "file" : "files"}`}
      </p>

      {loaded && entries.length === 0 ? (
        <section className="empty">
          <h2>{nothingKept ? "No files kept yet" : `No files named “${typed.trim()}”`}</h2>
          <p>Folyvo keeps files in the folders you choose.</p>
          {!nothingKept && (
            <button type="button" className="btn btn--primary" onClick={clearSearch}>
              Clear Search
            </button>
          )}
          <button
            type="button"
            className={nothingKept ? "btn btn--primary" : "btn btn--link"}
            disabled={act.busy}
            onClick={() => void addFolder()}
          >
            Add Folder…
          </button>
        </section>
      ) : (
        <div id={`${id}-list`} role="listbox" aria-label="Files" className="all__list">
          {named
            ? groups.map((g, n) => (
                <div key={g.path ?? "other"} role="group" aria-labelledby={`${id}-g-${n}`} className="all__group">
                  <h2 id={`${id}-g-${n}`} className="all__folder" title={g.path ?? undefined}>
                    {g.name}
                  </h2>
                  {g.files.map(row)}
                </div>
              ))
            : entries.map(row)}
        </div>
      )}
    </main>
  );
}
