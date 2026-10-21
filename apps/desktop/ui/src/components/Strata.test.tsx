import { render, screen, within } from "@testing-library/react";
import { Strata } from "./Strata";
import { version } from "../test-utils";

const now = Date.now();

test("layers are listed newest first and each names its version", () => {
  render(<Strata now={now} versions={[version({ seq: 3 }), version({ seq: 2 }), version({ seq: 1, origin: "initial" })]} />);
  const list = screen.getByRole("list", { name: "Versions, newest first" });
  const rows = within(list).getAllByRole("listitem");
  expect(rows.map((r) => r.textContent)).toEqual([
    expect.stringContaining("v3"),
    expect.stringContaining("v2"),
    expect.stringContaining("v1"),
  ]);
  expect(rows[2]).toHaveTextContent("First version");
});

test("the share-ready layer says so in words, not only in colour", () => {
  render(<Strata now={now} versions={[version({ seq: 2 }), version({ seq: 1, isShareReady: true })]} />);
  const ready = screen.getByText("Ready to share").closest(".band")!;
  expect(ready).toHaveTextContent("v1");
  expect(ready.className).toContain("band--ready");
});

test("bigger files make thicker layers", () => {
  render(<Strata now={now} versions={[version({ seq: 2, size: 900 * 1024 * 1024 }), version({ seq: 1, size: 2_000 })]} />);
  const [big, small] = Array.from(document.querySelectorAll<HTMLElement>(".band"));
  expect(parseInt(big.style.height)).toBeGreaterThan(parseInt(small.style.height));
});

test("a limit shows only the newest layers", () => {
  render(<Strata now={now} limit={2} versions={[version({ seq: 3 }), version({ seq: 2 }), version({ seq: 1 })]} />);
  expect(screen.getAllByRole("listitem")).toHaveLength(2);
});

test("the layer that was just marked ready carries the motion class", () => {
  const v = [version({ seq: 2, isShareReady: true }), version({ seq: 1 })];
  render(<Strata now={now} versions={v} justMarkedId={v[0].id} />);
  expect(screen.getByText("Ready to share").closest(".band")!.className).toContain("band--just-marked");
  expect(document.querySelectorAll(".band--just-marked")).toHaveLength(1);
});
