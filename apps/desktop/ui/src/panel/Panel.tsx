import { useState } from "react";
import { api, AppError } from "../api";
import { Band } from "../components/Band";
import { ErrorNotice } from "../components/ErrorNotice";
import { useAction, useHistory, useNow, useRecent } from "../queries";
import { exportLabel, markLabel } from "../strata";
import { NameField } from "./NameField";

export function Panel({ itemId }: { itemId: string }) {
  const now = useNow();
  const recent = useRecent();
  const history = useHistory(itemId);
  const act = useAction();
  const [naming, setNaming] = useState<string | null>(null);
  const [confirming, setConfirming] = useState<string | null>(null);
  const [notice, setNotice] = useState<{ text: string; reveal?: string } | null>(null);

  const summary = recent.data?.find((i) => i.itemId === itemId);
  const versions = history.data ?? [];
  const ready = versions.find((v) => v.isShareReady);

  const exportReady = async () => {
    if (!ready) return;
    const dir = await api.chooseFolder("Choose where to put the copy");
    if (!dir) return;
    const r = await act.run(() => api.exportVersion(itemId, ready.id, dir));
    if (r.ok) setNotice({ text: `Exported v${ready.seq}`, reveal: r.value.destination });
  };

  const saveName = async (id: string, value: string) => {
    await act.run(() => api.nameVersion(id, value.trim() === "" ? null : value));
    setNaming(null);
  };

  const restore = async (id: string, seq: number) => {
    setConfirming(null);
    const r = await act.run(() => api.restore(itemId, id));
    if (r.ok) setNotice({ text: `Restored v${seq} as v${r.value.seq}` });
  };

  const loadError = history.error instanceof AppError ? history.error.info : null;

  return (
    <main className="panel" aria-label="Version history">
      <h1>{summary?.name ?? "History"}</h1>
      {summary?.path && <p className="where">{summary.path}</p>}
      {summary?.missing && <p className="where">This file was removed from its folder. Its versions are still saved.</p>}

      {(act.error ?? loadError) && <ErrorNotice error={(act.error ?? loadError)!} onDismiss={act.clearError} />}
      {notice && !act.error && (
        <div role="status" className="notice" style={{ margin: "0 0 12px" }}>
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

      {ready && (
        <div className="panel__top">
          <button type="button" className="btn btn--primary" disabled={act.busy} onClick={() => void exportReady()}>
            {exportLabel(ready.seq)}
          </button>
        </div>
      )}

      <ul className="rows" style={{ listStyle: "none", margin: 0, padding: 0 }}>
        {versions.map((v, i) => (
          <li key={v.id} className="row" aria-label={`Version ${v.seq}`}>
            <Band version={v} index={i} now={now} />
            {naming === v.id ? (
              <NameField seq={v.seq} initial={v.note ?? ""} busy={act.busy} onSave={(value) => void saveName(v.id, value)} onCancel={() => setNaming(null)} />
            ) : (
              <div className="row__actions">
                <button type="button" className="btn" aria-label={`Name v${v.seq}`} onClick={() => setNaming(v.id)}>
                  Name
                </button>
                {!v.isShareReady && (
                  <button type="button" className="btn" disabled={act.busy} onClick={() => void act.run(() => api.markReady(itemId, v.id))}>
                    {markLabel(v.seq)}
                  </button>
                )}
                {(i > 0 || summary?.missing) && (
                  <button type="button" className="btn" disabled={act.busy} onClick={() => setConfirming(v.id)}>
                    Restore v{v.seq}
                  </button>
                )}
              </div>
            )}
            {confirming === v.id && (
              <div role="group" aria-label={`Confirm restore of v${v.seq}`} className="confirm">
                <p>Restore v{v.seq} as a new version? Folyvo saves your current file first, so nothing is lost.</p>
                <div className="row__actions">
                  <button type="button" className="btn btn--primary" onClick={() => void restore(v.id, v.seq)}>
                    Restore v{v.seq}
                  </button>
                  <button type="button" className="btn" onClick={() => setConfirming(null)}>
                    Cancel
                  </button>
                </div>
              </div>
            )}
          </li>
        ))}
      </ul>
    </main>
  );
}
