import { useEffect, useState } from "react";
import { api } from "../api";
import { ErrorNotice } from "../components/ErrorNotice";
import { folderName } from "../files";
import { useAction, useNow } from "../queries";
import { clock } from "../time";
import glyph from "../../../src-tauri/icons/tray.png";
import icon from "./folyvo-icon.svg";

interface Row {
  path: string;
  chosen: boolean;
}

/** What a new person sees once: what Folyvo does, which folders to keep, and where it lives afterwards. */
export function FirstWindow() {
  const now = useNow();
  const act = useAction();
  const [rows, setRows] = useState<Row[]>([]);

  useEffect(() => {
    void api.suggestedFolders().then((paths) => setRows(paths.map((path) => ({ path, chosen: false }))), () => {});
  }, []);

  const chosen = rows.filter((r) => r.chosen);

  const chooseAnother = async () => {
    const picked = await api.chooseFolder("Choose a folder to keep");
    if (!picked) return;
    setRows((now) =>
      now.some((r) => r.path === picked)
        ? now.map((r) => (r.path === picked ? { ...r, chosen: true } : r))
        : [...now, { path: picked, chosen: true }],
    );
  };

  const keep = async () => {
    const done = await act.run(async () => {
      for (const row of chosen) await api.addFolder(row.path);
    });
    if (done.ok) await act.run(() => api.finishFirstWindow());
  };

  return (
    <main className="first" aria-label="Welcome to Folyvo">
      <div className="first__drag" data-tauri-drag-region />
      <img className="first__icon" src={icon} alt="" width="56" height="56" />
      <h1>Every save, kept.</h1>
      <p className="first__what">
        Folyvo keeps every saved version of the files in the folders you choose, on this Mac.
        <br />
        Keep using Word.
      </p>

      {act.error && <ErrorNotice error={act.error} onDismiss={act.clearError} />}

      <section className="first__folders" aria-labelledby="first-folders">
        <h2 id="first-folders">Which folders should Folyvo keep?</h2>
        <ul>
          {rows.map((row) => (
            <li key={row.path}>
              <label title={row.path}>
                <input
                  type="checkbox"
                  checked={row.chosen}
                  disabled={act.busy}
                  onChange={(e) =>
                    setRows((now) => now.map((r) => (r.path === row.path ? { ...r, chosen: e.target.checked } : r)))
                  }
                />
                <span>{folderName(row.path)}</span>
              </label>
            </li>
          ))}
        </ul>
        <button type="button" className="first__link" disabled={act.busy} onClick={() => void chooseAnother()}>
          Choose Another Folder…
        </button>
        <p className="first__hint">
          Choose the folders you work in. Each saved version takes space on this Mac, and macOS asks once for each
          folder.
        </p>
      </section>

      <div className="first__actions">
        <button
          type="button"
          className="first__keep"
          disabled={act.busy || chosen.length === 0}
          onClick={() => void keep()}
        >
          Keep These Folders
        </button>
        <button
          type="button"
          className="first__link"
          disabled={act.busy}
          onClick={() => void act.run(() => api.finishFirstWindow())}
        >
          Not Now
        </button>
      </div>

      <footer className="first__where">
        <p>Folyvo stays in the menu bar. Its actions are also in Finder's right-click menu.</p>
        <div className="first__bar" aria-hidden="true">
          <img src={glyph} alt="" width="18" height="18" />
          <span>{clock(now)}</span>
        </div>
      </footer>
    </main>
  );
}
