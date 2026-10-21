import { useState } from "react";

interface Props {
  seq: number;
  initial: string;
  busy: boolean;
  onSave: (value: string) => void;
  onCancel: () => void;
}

export function NameField({ seq, initial, busy, onSave, onCancel }: Props) {
  const [value, setValue] = useState(initial);
  return (
    <form
      className="name-field"
      onSubmit={(e) => {
        e.preventDefault();
        onSave(value);
      }}
    >
      <input aria-label={`Name for v${seq}`} value={value} maxLength={200} autoFocus onChange={(e) => setValue(e.target.value)} />
      <button type="submit" className="btn btn--primary" disabled={busy}>
        Save name
      </button>
      <button type="button" className="btn" onClick={onCancel}>
        Cancel
      </button>
    </form>
  );
}
