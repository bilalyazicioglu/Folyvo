import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { LettingGoDto } from "../generated/LettingGoDto";
import type { SpaceDto } from "../generated/SpaceDto";
import { mockBackend, renderWithClient } from "../test-utils";
import { Space } from "./Space";

const MB = 1024 * 1024;

const space: SpaceDto = {
  bytes: 1200 * MB,
  files: [
    { itemId: "itm_01", name: "Teklif 2026.pptx", saves: 42, bytes: 610 * MB },
    { itemId: "itm_02", name: "Bütçe.xlsx", saves: 1, bytes: 240 * MB },
  ],
};

function backend(letting: Partial<LettingGoDto> = {}) {
  return mockBackend({
    handlers: {
      space: () => space,
      letting_go: () => ({ rule: "on", waiting: { saves: 0, bytes: 0 }, last: null, ...letting }),
    },
  });
}

test("tells the room Folyvo takes and the files that take the most", async () => {
  backend();
  renderWithClient(<Space />);
  expect(await screen.findByRole("heading", { name: "Folyvo uses 1.2 GB on this Mac" })).toBeInTheDocument();
  const rows = within(screen.getByRole("list", { name: "Takes the most" })).getAllByRole("button");
  expect(rows).toHaveLength(2);
  for (const text of ["Teklif 2026.pptx", "42 saves", "610.0 MB"]) expect(rows[0]).toHaveTextContent(text);
  for (const text of ["Bütçe.xlsx", "1 save", "240.0 MB"]) expect(rows[1]).toHaveTextContent(text);
});

test("a file opens its history", async () => {
  const b = backend();
  renderWithClient(<Space />);
  await userEvent.click(await screen.findByRole("button", { name: /Teklif 2026\.pptx/ }));
  expect(b.called("open_history")[0].args).toEqual({ item: "itm_01" });
});

test("the rule can be turned off and on again", async () => {
  const b = backend();
  renderWithClient(<Space />);
  const toggle = await screen.findByRole("checkbox", { name: "Let go of older saves" });
  expect(toggle).toBeChecked();
  await userEvent.click(toggle);
  expect(b.called("set_letting_go")[0].args).toEqual({ on: false });
});

test("before it was asked, the rule says nothing has gone yet", async () => {
  backend({ rule: "unasked" });
  renderWithClient(<Space />);
  expect(await screen.findByRole("checkbox", { name: "Let go of older saves" })).not.toBeChecked();
  expect(screen.getByText("Folyvo asks before it first lets a save go.")).toBeInTheDocument();
});

test("says when older saves were last let go", async () => {
  backend({ last: { at: new Date(2026, 10, 20, 9, 0).getTime(), saves: 312, bytes: 2.4 * 1024 * MB } });
  renderWithClient(<Space />);
  expect(await screen.findByText("Let go of 312 saves, 2.4 GB, on 20 November.")).toBeInTheDocument();
});

test("with nothing kept yet, says so", async () => {
  mockBackend();
  renderWithClient(<Space />);
  await waitFor(() => expect(screen.getByRole("heading", { name: "Folyvo uses 0 B on this Mac" })).toBeInTheDocument());
  expect(screen.getByText("Nothing takes room yet.")).toBeInTheDocument();
});
