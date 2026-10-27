import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
  type PointerEvent,
} from "react";
import { DocIcon } from "../components/DocIcon";
import type { FileKind } from "../files";
import type { PreviewDto } from "../generated/PreviewDto";

/** How far a page must travel before letting go throws it instead of springing back. */
export const THROW_PX = 120;
/** How long a thrown page stays on screen while it flies off. */
const FLIGHT_MS = 380;

interface Props {
  name: string;
  kind: FileKind;
  when: string;
  meta: string;
  preview: PreviewDto | undefined;
  /** The picture could not be asked for at all; shown like a kind Quick Look cannot draw. */
  failed: boolean;
  /** How many saves lie under this one, and over it. */
  older: number;
  newer: number;
  onOlder: () => void;
  onNewer: () => void;
}

type Dir = 1 | -1;

/**
 * A picture of the file at one time, on top of a pile of the saves before it. Throwing the page
 * aside shows the save under it; pulling the other way brings the newer one back. Evidence, never an editor.
 */
export function Stage({ name, kind, when, meta, preview, failed, older, newer, onOlder, onNewer }: Props) {
  const [dx, setDx] = useState(0);
  const from = useRef<number | null>(null);
  const [thrown, setThrown] = useState<{
    src: string | null;
    text: string | null;
    dx: number;
    key: number;
    gone: boolean;
  } | null>(null);
  const [arriving, setArriving] = useState(false);
  const cameBack = useRef(false);
  const picture = preview?.picture ?? null;

  const go = (dir: Dir, at = 0) => {
    if (dir === 1 ? older === 0 : newer === 0) return;
    const text = preview?.text ?? null;
    if (dir === 1 && (picture || text)) setThrown({ src: picture, text, dx: at, key: Date.now(), gone: false });
    cameBack.current = dir === -1;
    if (dir === 1) onOlder();
    else onNewer();
  };

  // A thrown page leaves the screen on its own; it is only drawn while it flies.
  const flight = thrown?.key;
  useEffect(() => {
    if (flight === undefined) return;
    const f = requestAnimationFrame(() => setThrown((t) => t && { ...t, gone: true }));
    const t = setTimeout(() => setThrown(null), FLIGHT_MS);
    return () => (cancelAnimationFrame(f), clearTimeout(t));
  }, [flight]);

  // A newer save slides back onto the pile from the left.
  useLayoutEffect(() => {
    if (!cameBack.current) return;
    cameBack.current = false;
    setArriving(true);
    const f = requestAnimationFrame(() => setArriving(false));
    return () => cancelAnimationFrame(f);
  }, [when]);

  const down = (e: PointerEvent) => {
    if (e.button !== 0) return;
    from.current = e.clientX;
    e.currentTarget.setPointerCapture?.(e.pointerId);
  };
  const move = (e: PointerEvent) => {
    if (from.current === null) return;
    const pulled = e.clientX - from.current;
    const blocked = pulled > 0 ? older === 0 : newer === 0;
    setDx(blocked ? pulled / 6 : pulled);
  };
  const up = (e: PointerEvent) => {
    if (from.current === null) return;
    const travelled = e.clientX - from.current;
    from.current = null;
    setDx(0);
    if (travelled > THROW_PX) go(1, travelled);
    else if (travelled < -THROW_PX) go(-1);
  };
  const key = (e: KeyboardEvent) => {
    const dir = e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? -1 : 0;
    if (!dir) return;
    e.preventDefault();
    go(dir);
  };

  const dragging = dx !== 0;
  const top = arriving
    ? { transform: "translateX(-130%) rotate(-14deg)", transition: "none" }
    : dragging
      ? { transform: `translateX(${dx}px) rotate(${dx / 24}deg)`, transition: "none" }
      : undefined;

  return (
    <section className={`stage stage--${kind}`} aria-label="Picture">
      <div className="stage__glow" aria-hidden="true" />
      <div className="stage__row">
        <button
          type="button"
          className="stage__step"
          aria-label="Newer save"
          disabled={newer === 0}
          onClick={() => go(-1)}
        >
          <svg viewBox="0 0 16 16" aria-hidden="true">
            <path d="M10 3 5 8l5 5" />
          </svg>
        </button>
        <div
          className={`stage__pile${dragging ? " is-dragging" : ""}`}
          role="group"
          aria-label={`Saved pages, showing ${when}. Drag the page or use the arrow keys for older and newer saves.`}
          tabIndex={0}
          onKeyDown={key}
          onPointerDown={down}
          onPointerMove={move}
          onPointerUp={up}
          onPointerCancel={() => ((from.current = null), setDx(0))}
        >
          {Array.from({ length: Math.min(3, older) }, (_, i) => (
            <div key={i} className={`stage__page stage__page--${i + 1}`} aria-hidden="true" />
          ))}
          <div className="stage__top" style={top}>
            {preview?.text ? (
              <div className="stage__paper" role="img" aria-label={`Picture of ${name} as it was ${when}`}>
                <pre aria-hidden="true">{preview.text}</pre>
              </div>
            ) : picture ? (
              <img
                className="stage__picture"
                src={picture}
                alt={`Picture of ${name} as it was ${when}`}
                draggable={false}
              />
            ) : preview || failed ? (
              <div className="stage__none">
                <DocIcon name={name} big />
                <p>No preview for this kind of file.</p>
              </div>
            ) : (
              <div className="stage__none" aria-busy="true" />
            )}
          </div>
          {thrown && (
            <div
              key={thrown.key}
              className={`stage__thrown${thrown.gone ? " is-gone" : ""}`}
              aria-hidden="true"
              style={{ "--from": `${thrown.dx}px`, "--turn": `${thrown.dx / 24}deg` } as CSSProperties}
            >
              {thrown.text !== null ? (
                <div className="stage__paper">
                  <pre>{thrown.text}</pre>
                </div>
              ) : (
                <img className="stage__picture" src={thrown.src ?? undefined} alt="" />
              )}
            </div>
          )}
        </div>
        <button
          type="button"
          className="stage__step"
          aria-label="Older save"
          disabled={older === 0}
          onClick={() => go(1)}
        >
          <svg viewBox="0 0 16 16" aria-hidden="true">
            <path d="m6 3 5 5-5 5" />
          </svg>
        </button>
      </div>
      <p className="stage__meta">{meta}</p>
    </section>
  );
}
