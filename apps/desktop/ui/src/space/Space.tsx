import { useId } from "react";
import { api } from "../api";
import { DocIcon } from "../components/DocIcon";
import { ErrorNotice } from "../components/ErrorNotice";
import { sizeLabel } from "../files";
import { useAction, useLettingGo, useSpace, useUpdates } from "../queries";

/** How many files the window lists. */
const TOP = 10;

function lastLetGo(at: number, saves: number, bytes: number): string {
  const day = new Date(at).toLocaleDateString("en-GB", { day: "numeric", month: "long" });
  return `Let go of ${saves} ${saves === 1 ? "save" : "saves"}, ${sizeLabel(bytes)}, on ${day}.`;
}

/** How much room Folyvo takes on this Mac, what takes the most, and the rule for older saves. */
export function Space() {
  const space = useSpace(TOP);
  const letting = useLettingGo();
  const updates = useUpdates();
  const act = useAction();
  const top = useId();
  const older = useId();
  const rule = letting.data?.rule;
  const last = letting.data?.last;

  return (
    <main className="space" aria-label="Space" aria-busy={space.isPending}>
      <header className="space__head" data-tauri-drag-region />
      {act.error && <ErrorNotice error={act.error} onDismiss={act.clearError} />}
      {space.data && (
        <div className="space__body">
          <h1>Folyvo uses {sizeLabel(space.data.bytes)} on this Mac</h1>
          <p className="space__hint">Files that never changed take no extra room.</p>

          <h2 id={top}>Takes the most</h2>
          {space.data.files.length === 0 ? (
            <p className="space__hint">Nothing takes room yet.</p>
          ) : (
            <ul className="space__files" aria-labelledby={top}>
              {space.data.files.map((f) => (
                <li key={f.itemId}>
                  <button type="button" className="space__file" onClick={() => void api.openHistory(f.itemId)}>
                    <DocIcon name={f.name} />
                    <span className="space__name" title={f.name}>
                      {f.name}
                    </span>
                    <span className="space__saves">
                      {f.saves} {f.saves === 1 ? "save" : "saves"}
                    </span>
                    <span className="space__bytes">{sizeLabel(f.bytes)}</span>
                  </button>
                </li>
              ))}
            </ul>
          )}

          <h2 id={older}>Older saves</h2>
          <p className="space__rule">
            Every save from the last 14 days, then one a day. Saves you marked, sent or wrote a note on always stay.
          </p>
          {rule && (
            <label className="space__toggle">
              <input
                type="checkbox"
                checked={rule === "on"}
                disabled={act.busy}
                onChange={(e) => void act.run(() => api.setLettingGo(e.target.checked))}
              />
              Let go of older saves
            </label>
          )}
          {rule === "unasked" && <p className="space__hint">Folyvo asks before it first lets a save go.</p>}
          {last && <p className="space__hint">{lastLetGo(last.at, last.saves, last.bytes)}</p>}

          {updates.data && (
            <>
              <h2>Updates</h2>
              <label className="space__toggle">
                <input
                  type="checkbox"
                  checked={updates.data.checks}
                  disabled={act.busy}
                  onChange={(e) => void act.run(() => api.setUpdateChecks(e.target.checked))}
                />
                Check for updates once a day
              </label>
              <p className="space__hint">
                Folyvo asks GitHub for the latest version and sends nothing else. This is Folyvo {updates.data.current}.
              </p>
              {updates.data.needsMove && <p className="space__hint">Move Folyvo to Applications to get updates.</p>}
            </>
          )}
        </div>
      )}
    </main>
  );
}
