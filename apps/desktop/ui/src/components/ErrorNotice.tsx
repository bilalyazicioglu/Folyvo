import type { UserError } from "../generated/UserError";

export function ErrorNotice({ error, onDismiss }: { error: UserError; onDismiss: () => void }) {
  return (
    <div role="alert" className="notice notice--error">
      <div>
        <strong>{error.title}</strong>
        <p>{error.hint}</p>
      </div>
      <button type="button" className="btn btn--link" onClick={onDismiss}>
        Dismiss
      </button>
    </div>
  );
}
