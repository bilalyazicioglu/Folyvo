import type { FileKind } from "../files";
import type { PreviewDto } from "../generated/PreviewDto";

interface Props {
  name: string;
  kind: FileKind;
  when: string;
  meta: string;
  preview: PreviewDto | undefined;
  /** How many older saves lie behind this one; up to three faint pages are drawn. */
  older: number;
}

/** A picture of the file at one time, on a neutral stage. Evidence, never an editor. */
export function Stage({ name, kind, when, meta, preview, older }: Props) {
  return (
    <section className={`stage stage--${kind}`} aria-label="Picture">
      <div className="stage__glow" aria-hidden="true" />
      <div className="stage__sheet">
        {Array.from({ length: Math.min(3, older) }, (_, i) => (
          <div key={i} className={`stage__page stage__page--${i + 1}`} aria-hidden="true" />
        ))}
        {preview?.picture ? (
          <img className="stage__picture" src={preview.picture} alt={`Picture of ${name} as it was ${when}`} draggable={false} />
        ) : preview ? (
          <div className="stage__none">
            <span className={`tile tile--big tile--${kind}`} aria-hidden="true" />
            <p>No preview for this kind of file.</p>
          </div>
        ) : (
          <div className="stage__none" aria-busy="true" />
        )}
      </div>
      <p className="stage__meta">{meta}</p>
    </section>
  );
}
