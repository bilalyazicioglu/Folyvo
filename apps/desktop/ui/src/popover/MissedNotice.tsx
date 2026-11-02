/** Files that changed while Folyvo was not running: their last state is kept, the saves between are not. */
export function MissedNotice({ count, onDismiss }: { count: number; onDismiss: () => void }) {
  const one = count === 1;
  return (
    <div role="status" className="notice">
      <div>
        <strong>
          {count} {one ? "file" : "files"} changed while Folyvo wasn't running
        </strong>
        <p>Folyvo kept how {one ? "it looks" : "they look"} now, not the saves in between.</p>
      </div>
      <button type="button" className="btn btn--link" onClick={onDismiss}>
        OK
      </button>
    </div>
  );
}
