import { useId } from "react";
import { sizeLabel } from "../files";

/** Asked once, the first time the rule for older saves would let any go. Nothing goes before an answer. */
export function LetGoQuestion({ bytes, onAnswer, busy }: { bytes: number; onAnswer: (on: boolean) => void; busy: boolean }) {
  const title = useId();
  return (
    <section className="notice notice--question" aria-labelledby={title}>
      <div>
        <strong id={title}>Folyvo will start letting go of older saves</strong>
        <p>
          It keeps every save from the last 14 days, then one a day. Saves you marked, sent or wrote a note on always
          stay. This frees up to {sizeLabel(bytes)}.
        </p>
        <div className="notice__actions">
          <button type="button" className="btn btn--link" disabled={busy} onClick={() => onAnswer(false)}>
            Keep Every Save
          </button>
          <button type="button" className="btn btn--primary" disabled={busy} onClick={() => onAnswer(true)}>
            OK
          </button>
        </div>
      </div>
    </section>
  );
}
