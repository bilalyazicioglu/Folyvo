import { fileKind } from "../files";

/** The extension as people see it on a file, at most four letters: "DOCX", "KEY". */
export function extLabel(name: string): string {
  const dot = name.lastIndexOf(".");
  const ext = dot > 0 ? name.slice(dot + 1) : "";
  return /^[a-z0-9]{1,4}$/i.test(ext) ? ext.toUpperCase() : "";
}

/** A small sheet of paper with the file's extension on a band in its kind's colour. */
export function DocIcon({ name, big = false }: { name: string; big?: boolean }) {
  return (
    <span className={`doc doc--${fileKind(name)}${big ? " doc--big" : ""}`} aria-hidden="true">
      <b>{extLabel(name)}</b>
    </span>
  );
}
