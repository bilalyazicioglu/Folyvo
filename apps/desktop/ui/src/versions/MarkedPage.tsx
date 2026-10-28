import { useEffect, useRef } from "react";
import type { MarkedBlockDto } from "../generated/MarkedBlockDto";
import type { MarkedPageDto } from "../generated/MarkedPageDto";
import type { SectionDto } from "../generated/SectionDto";
import type { SpanDto } from "../generated/SpanDto";

const SECTION: Record<SectionDto, string | null> = { body: null, header: "Header", footer: "Footer", notes: "Notes" };

/** Words as they read now: added ones highlighted, removed ones struck through where they were. */
function Spans({ spans }: { spans: SpanDto[] }) {
  return (
    <>
      {spans.map((s, i) =>
        s.kind === "added" ? <ins key={i}>{s.text}</ins> : s.kind === "removed" ? <del key={i}>{s.text}</del> : s.text,
      )}
    </>
  );
}

function Block({ block }: { block: MarkedBlockDto }) {
  if (block.kind === "table") {
    return (
      <table className="marked__table" data-mark={block.mark}>
        <tbody>
          {block.rows.map((row, r) => (
            <tr key={r}>
              {row.map((cell, c) => (
                <td key={c} data-mark={cell.mark}>
                  <Spans spans={cell.spans} />
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    );
  }
  const words = <Spans spans={block.spans} />;
  if (block.style.kind === "heading") {
    return block.style.level <= 1 ? <h2 data-mark={block.mark}>{words}</h2> : <h3 data-mark={block.mark}>{words}</h3>;
  }
  return (
    <p data-mark={block.mark} className={block.style.kind === "listItem" ? "marked__item" : undefined}>
      {words}
    </p>
  );
}

/**
 * A save's words on plain paper, with what changed since the save before marked the way Word marks
 * tracked changes: a bar in the margin, new words highlighted, removed words struck through.
 * Something to read, never to edit.
 */
export function MarkedPage({ page, when }: { page: MarkedPageDto | undefined; when: string }) {
  const paper = useRef<HTMLDivElement>(null);

  // Opening the page brings the first change into view.
  useEffect(() => {
    const first = paper.current?.querySelector('[data-mark="added"], [data-mark="removed"], [data-mark="changed"]');
    first?.scrollIntoView?.({ block: "center" });
  }, [page]);

  let body;
  if (!page) {
    body = <p className="marked__quiet" aria-busy="true" />;
  } else if (page.kind === "unreadable") {
    body = <p className="marked__quiet">Folyvo can't read the words in this file.</p>;
  } else if (page.kind === "identical") {
    body = <p className="marked__quiet">Same as the save before.</p>;
  } else if (page.kind === "text") {
    body = page.blocks.map((block, i) => {
      const label = SECTION[block.section];
      const starts = label && (i === 0 || page.blocks[i - 1].section !== block.section);
      return (
        <div key={i} className="marked__block">
          {starts && <p className="marked__section">{label}</p>}
          <Block block={block} />
        </div>
      );
    });
  } else if (page.kind === "sheets") {
    body = page.sheets.map((sheet) => (
      <section key={sheet.name} className="marked__sheet">
        <h2 data-mark={sheet.mark}>{sheet.name}</h2>
        {sheet.mark === "added" ? (
          <p className="marked__quiet">Sheet added</p>
        ) : sheet.mark === "removed" ? (
          <p className="marked__quiet">Sheet removed</p>
        ) : sheet.cells.length === 0 ? (
          <p className="marked__quiet">No cells changed</p>
        ) : (
          <table className="marked__cells">
            <thead>
              <tr>
                <th scope="col">Cell</th>
                <th scope="col">Before</th>
                <th scope="col">After</th>
              </tr>
            </thead>
            <tbody>
              {sheet.cells.map((c) => (
                <tr key={c.at} data-mark={c.before === null ? "added" : c.after === null ? "removed" : "changed"}>
                  <th scope="row">{c.at}</th>
                  <td>{c.before !== null && <del>{c.before}</del>}</td>
                  <td>{c.after !== null && <ins>{c.after}</ins>}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </section>
    ));
  } else {
    const changed = page.pages.filter((p) => p.mark !== "same");
    body = changed.map((p) => (
      <section key={p.number} className="marked__sheet">
        <h2 data-mark={p.mark}>Page {p.number}</h2>
        <p data-mark={p.mark}>
          <Spans spans={p.spans} />
        </p>
      </section>
    ));
  }

  return (
    <div ref={paper} className="stage__paper marked" role="document" aria-label={`What changed in ${when}`}>
      {body}
    </div>
  );
}
