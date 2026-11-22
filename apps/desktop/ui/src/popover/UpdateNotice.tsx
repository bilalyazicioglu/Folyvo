import { useId } from "react";
import type { UpdatesDto } from "../generated/UpdatesDto";

/** A newer Folyvo found by the daily look. Nothing installs until Install and Restart. */
export function UpdateNotice({ updates, onInstall, busy }: { updates: UpdatesDto; onInstall: () => void; busy: boolean }) {
  const title = useId();
  const version = updates.ready;
  if (!version) return null;
  return (
    <section className="notice notice--question" aria-labelledby={title}>
      <div>
        <strong id={title}>{updates.failed ? `Folyvo could not install ${version}` : `Folyvo ${version} is ready`}</strong>
        <p>
          {updates.failed
            ? "Nothing was changed. Check the connection and try again."
            : "Folyvo closes for a moment and opens again. Your saves stay as they are."}
        </p>
        <div className="notice__actions">
          <button type="button" className="btn btn--primary" disabled={busy || updates.installing} onClick={onInstall}>
            {updates.installing ? "Installing…" : updates.failed ? "Try Again" : "Install and Restart"}
          </button>
        </div>
      </div>
    </section>
  );
}
