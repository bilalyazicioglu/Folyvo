import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { item, mockBackend, renderWithClient, version } from "../test-utils";
import { Panel } from "./Panel";

const history = [
  version({ seq: 4 }),
  version({ seq: 3, isShareReady: true, note: "Sent to Acme" }),
  version({ seq: 2 }),
  version({ seq: 1, origin: "initial" }),
];

function backend(over: Parameters<typeof mockBackend>[0] = {}) {
  return mockBackend({
    recent: [item({ status: "changedSinceReady", latestSeq: 4 })],
    history: { itm_01: history },
    ...over,
  });
}

function row(seq: number) {
  return screen.getByRole("listitem", { name: `Version ${seq}` });
}

test("every version is a row, newest first, with the name and path of the file on top", async () => {
  backend();
  renderWithClient(<Panel itemId="itm_01" />);
  expect(await screen.findByRole("heading", { name: "Proposal_Acme.docx" })).toBeInTheDocument();
  expect(screen.getByText("/Users/a/Client work/Proposal_Acme.docx")).toBeInTheDocument();
  const rows = screen.getAllByRole("listitem", { name: /^Version/ });
  expect(rows.map((r) => r.getAttribute("aria-label"))).toEqual(["Version 4", "Version 3", "Version 2", "Version 1"]);
});

test("actions depend on the row: no restore on the newest, no mark on the ready one", async () => {
  backend();
  renderWithClient(<Panel itemId="itm_01" />);
  await screen.findByRole("heading", { name: "Proposal_Acme.docx" });
  expect(within(row(4)).queryByRole("button", { name: "Restore v4" })).not.toBeInTheDocument();
  expect(within(row(4)).getByRole("button", { name: "Mark v4 ready" })).toBeInTheDocument();
  expect(within(row(3)).queryByRole("button", { name: "Mark v3 ready" })).not.toBeInTheDocument();
  expect(within(row(3)).getByRole("button", { name: "Restore v3" })).toBeInTheDocument();
});

test("marking an older version ready sends that version", async () => {
  const b = backend();
  renderWithClient(<Panel itemId="itm_01" />);
  await userEvent.click(await within(await screen.findByRole("listitem", { name: "Version 2" })).findByRole("button", { name: "Mark v2 ready" }));
  await waitFor(() => expect(b.called("mark_ready")).toHaveLength(1));
  expect(b.called("mark_ready")[0].args).toEqual({ item: "itm_01", version: "ver_02" });
});

// Review Focus 4: restore must explain that unsaved work is kept, and never run on the first click.
test("restore asks first, explains that nothing is lost, and only then restores", async () => {
  const b = backend();
  b.state.handlers.restore = () => ({ versionId: "ver_05", seq: 5 });
  renderWithClient(<Panel itemId="itm_01" />);
  await userEvent.click(await within(await screen.findByRole("listitem", { name: "Version 2" })).findByRole("button", { name: "Restore v2" }));
  const confirm = screen.getByRole("group", { name: "Confirm restore of v2" });
  expect(confirm).toHaveTextContent("Restore v2 as a new version? Folyvo saves your current file first, so nothing is lost.");
  expect(b.called("restore")).toHaveLength(0);

  await userEvent.click(within(confirm).getByRole("button", { name: "Cancel" }));
  expect(screen.queryByRole("group", { name: "Confirm restore of v2" })).not.toBeInTheDocument();
  expect(b.called("restore")).toHaveLength(0);

  await userEvent.click(within(row(2)).getByRole("button", { name: "Restore v2" }));
  await userEvent.click(within(screen.getByRole("group", { name: "Confirm restore of v2" })).getByRole("button", { name: "Restore v2" }));
  await waitFor(() => expect(b.called("restore")).toHaveLength(1));
  expect(b.called("restore")[0].args).toEqual({ item: "itm_01", version: "ver_02" });
  expect(await screen.findByText("Restored v2 as v5")).toBeInTheDocument();
});

test("a version can be named and the name can be cleared", async () => {
  const b = backend();
  renderWithClient(<Panel itemId="itm_01" />);
  await userEvent.click(await within(await screen.findByRole("listitem", { name: "Version 2" })).findByRole("button", { name: "Name v2" }));
  await userEvent.type(screen.getByRole("textbox", { name: "Name for v2" }), "Pricing section added");
  await userEvent.click(screen.getByRole("button", { name: "Save name" }));
  await waitFor(() => expect(b.called("name_version")).toHaveLength(1));
  expect(b.called("name_version")[0].args).toEqual({ version: "ver_02", note: "Pricing section added" });

  await userEvent.click(within(row(2)).getByRole("button", { name: "Name v2" }));
  await userEvent.clear(screen.getByRole("textbox", { name: "Name for v2" }));
  await userEvent.click(screen.getByRole("button", { name: "Save name" }));
  await waitFor(() => expect(b.called("name_version")).toHaveLength(2));
  expect(b.called("name_version")[1].args).toEqual({ version: "ver_02", note: null });
});

test("the ready version can be exported from the top of the panel", async () => {
  const b = backend({ picked: "/Users/a/Sent" });
  b.state.handlers.export = () => ({ destination: "/Users/a/Sent/Proposal_Acme.docx", versionId: "ver_03" });
  renderWithClient(<Panel itemId="itm_01" />);
  await userEvent.click(await screen.findByRole("button", { name: "Export v3" }));
  expect(await screen.findByText("Exported v3")).toBeInTheDocument();
  expect(b.called("export")[0].args).toEqual({ item: "itm_01", expected: "ver_03", destDir: "/Users/a/Sent" });
});

test("a removed file can be brought back from its newest version", async () => {
  backend({ recent: [item({ missing: true, latestSeq: 4 })] });
  renderWithClient(<Panel itemId="itm_01" />);
  await screen.findByRole("heading", { name: "Proposal_Acme.docx" });
  expect(within(row(4)).getByRole("button", { name: "Restore v4" })).toBeInTheDocument();
  expect(screen.getByText("This file was removed from its folder. Its versions are still saved.")).toBeInTheDocument();
});

test("a backend error is shown with its title and hint", async () => {
  const b = backend();
  b.state.handlers.mark_ready = () => {
    throw { code: "item_missing", title: "Folyvo can't find this file", hint: "It may have been removed from a watched folder." };
  };
  renderWithClient(<Panel itemId="itm_01" />);
  await userEvent.click(await within(await screen.findByRole("listitem", { name: "Version 2" })).findByRole("button", { name: "Mark v2 ready" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Folyvo can't find this file");
});

test("an item the vault does not know shows the reason instead of an empty window", async () => {
  const b = backend({ recent: [], history: {} });
  b.state.handlers.history = () => {
    throw { code: "item_missing", title: "Folyvo can't find this file", hint: "It may have been removed from a watched folder." };
  };
  renderWithClient(<Panel itemId="itm_99" />);
  expect(await screen.findByRole("alert")).toHaveTextContent("It may have been removed from a watched folder.");
});
