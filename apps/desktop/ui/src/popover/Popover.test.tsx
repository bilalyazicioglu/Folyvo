import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { item, mockBackend, renderWithClient, version } from "../test-utils";
import { Popover } from "./Popover";

const changedHistory = [
  version({ seq: 9, note: null }),
  version({ seq: 8, isShareReady: true, note: "Sent to Acme for review" }),
  version({ seq: 7 }),
  version({ seq: 6, note: "Pricing section added" }),
  version({ seq: 5 }),
];

function backend(over: Parameters<typeof mockBackend>[0] = {}) {
  return mockBackend({
    recent: [item({ status: "changedSinceReady", latestSeq: 9 })],
    history: { itm_01: changedHistory },
    stats: { items: 1, versions: 9, bytes: 3 * 1024 * 1024 },
    ...over,
  });
}

test("a file that changed since it was marked ready offers both actions and says so", async () => {
  backend();
  renderWithClient(<Popover />);
  expect(await screen.findByRole("heading", { name: "Proposal_Acme.docx" })).toBeInTheDocument();
  expect(await screen.findByText("Changed since v8, your share-ready version.")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Export v8" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Mark v9 ready" })).toBeInTheDocument();
  expect(screen.getByText("Watching Client work")).toBeInTheDocument();
  expect(screen.getByText("Stored on this Mac, 3.0 MB, 9 versions")).toBeInTheDocument();
});

test("the card shows the four newest layers only", async () => {
  backend();
  renderWithClient(<Popover />);
  const layers = await within(await screen.findByRole("list", { name: "Versions, newest first" })).findAllByRole("listitem");
  expect(layers).toHaveLength(4);
});

test("a file that was never shared can only be marked ready", async () => {
  backend({ recent: [item({ status: "notShared", latestSeq: 1 })], history: { itm_01: [version({ seq: 1, origin: "initial" })] } });
  renderWithClient(<Popover />);
  expect(await screen.findByRole("button", { name: "Mark v1 ready" })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /^Export/ })).not.toBeInTheDocument();
});

test("a file whose latest version is ready offers export only", async () => {
  backend({
    recent: [item({ status: "ready", latestSeq: 8 })],
    history: { itm_01: [version({ seq: 8, isShareReady: true }), version({ seq: 7 })] },
  });
  renderWithClient(<Popover />);
  expect(await screen.findByRole("button", { name: "Export v8" })).toBeInTheDocument();
  expect(screen.getByText("v8 is ready to share.")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /^Mark/ })).not.toBeInTheDocument();
});

test("marking a version ready sends that exact version", async () => {
  const b = backend();
  renderWithClient(<Popover />);
  await userEvent.click(await screen.findByRole("button", { name: "Mark v9 ready" }));
  await waitFor(() => expect(b.called("mark_ready")).toHaveLength(1));
  expect(b.called("mark_ready")[0].args).toEqual({ item: "itm_01", version: "ver_09" });
});

test("export asks where to put the copy, then exports the ready version and offers Finder", async () => {
  const b = backend({ picked: "/Users/a/Sent" });
  b.state.handlers.export = () => ({ destination: "/Users/a/Sent/Proposal_Acme.docx", versionId: "ver_08" });
  renderWithClient(<Popover />);
  await userEvent.click(await screen.findByRole("button", { name: "Export v8" }));
  expect(await screen.findByText("Exported v8")).toBeInTheDocument();
  expect(b.called("export")[0].args).toEqual({ item: "itm_01", expected: "ver_08", destDir: "/Users/a/Sent" });
  await userEvent.click(screen.getByRole("button", { name: "Show in Finder" }));
  expect(b.called("reveal")[0].args).toEqual({ path: "/Users/a/Sent/Proposal_Acme.docx" });
});

test("cancelling the folder picker exports nothing", async () => {
  const b = backend({ picked: null });
  renderWithClient(<Popover />);
  await userEvent.click(await screen.findByRole("button", { name: "Export v8" }));
  await waitFor(() => expect(b.called("plugin:dialog|open")).toHaveLength(1));
  expect(b.called("export")).toHaveLength(0);
});

// Review Focus 3: the ready version changed between rendering and clicking.
test("an export that the backend refuses explains what happened and what to do", async () => {
  const b = backend({ picked: "/Users/a/Sent" });
  b.state.handlers.export = () => {
    throw { code: "share_ready_changed", title: "The ready version changed", hint: "Look at the current ready version and export again." };
  };
  renderWithClient(<Popover />);
  await userEvent.click(await screen.findByRole("button", { name: "Export v8" }));
  const alert = await screen.findByRole("alert");
  expect(alert).toHaveTextContent("The ready version changed");
  expect(alert).toHaveTextContent("Look at the current ready version and export again.");
  await userEvent.click(within(alert).getByRole("button", { name: "Dismiss" }));
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});

test("a removed file says so and only offers its history", async () => {
  backend({ recent: [item({ missing: true, status: "notShared", latestSeq: 3 })], history: { itm_01: [version({ seq: 3 })] } });
  renderWithClient(<Popover />);
  expect(await screen.findByText("This file was removed from its folder. Its versions are still saved.")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /^(Export|Mark)/ })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "All versions" })).toBeInTheDocument();
});

// Review Focus 7: long, spaced and non-Latin names.
test("a long name with spaces and non-Latin characters is shown whole to assistive technology", async () => {
  const name = "Çok uzun bir dosya adı — final sürüm (2) 日本語のファイル名.docx";
  backend({ recent: [item({ name })], history: { itm_01: [version({ seq: 1 })] } });
  renderWithClient(<Popover />);
  const card = await screen.findByRole("region", { name });
  expect(within(card).getByRole("heading", { name })).toBeInTheDocument();
});

test("other recent files are listed and open their own history", async () => {
  const b = backend({
    recent: [item(), item({ itemId: "itm_02", name: "Budget_2027.xlsx", latestAt: Date.now() - 3_600_000 })],
    history: { itm_01: [version({ seq: 1 })] },
  });
  renderWithClient(<Popover />);
  await userEvent.click(await screen.findByRole("button", { name: /Budget_2027\.xlsx/ }));
  expect(b.called("open_history")[0].args).toEqual({ item: "itm_02" });
});

test("All versions opens the history of the current file", async () => {
  const b = backend();
  renderWithClient(<Popover />);
  await userEvent.click(await screen.findByRole("button", { name: "All versions" }));
  expect(b.called("open_history")[0].args).toEqual({ item: "itm_01" });
});

// Review Focus 5
test("the first launch shows the invitation and adding a folder starts watching it", async () => {
  const b = mockBackend({ folders: [], recent: [], picked: "/Users/a/Client work" });
  renderWithClient(<Popover />);
  await userEvent.click(await screen.findByRole("button", { name: "Choose folder" }));
  await waitFor(() => expect(b.called("add_folder")).toHaveLength(1));
  expect(b.called("add_folder")[0].args).toEqual({ path: "/Users/a/Client work" });
});

test("a watched folder with no saved files yet says it is waiting", async () => {
  mockBackend({ recent: [] });
  renderWithClient(<Popover />);
  expect(await screen.findByText("Save a file in Client work and it shows up here.")).toBeInTheDocument();
});

test("several folders are summarised in the header", async () => {
  mockBackend({
    folders: [
      { path: "/Users/a/Client work", problem: null },
      { path: "/Users/a/Taxes", problem: null },
      { path: "/Users/a/Notes", problem: null },
    ],
  });
  renderWithClient(<Popover />);
  expect(await screen.findByText("Watching 3 folders")).toBeInTheDocument();
});

test("Escape closes the popover", async () => {
  const b = backend();
  renderWithClient(<Popover />);
  await screen.findByRole("heading", { name: "Proposal_Acme.docx" });
  await userEvent.keyboard("{Escape}");
  await waitFor(() => expect(b.called("hide_popover")).toHaveLength(1));
});
