import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { api, type DropEvent } from "../api";
import type { CopyDto } from "../generated/CopyDto";
import { item, mockBackend, renderWithClient, version } from "../test-utils";
import { Popover } from "./Popover";

const at = (d: number, h: number, m: number) => new Date(2026, 9, d, h, m).getTime();
const NOW = at(26, 10, 42);
const LONG = "Teklif_Müşteri_2026_son_hâli.docx";

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(NOW);
});
afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

const handed = (itemId: string, versionId: string, when: number): CopyDto => ({
  itemId,
  versionId,
  kind: "handed",
  service: "mail",
  targetPath: null,
  at: when,
});

/** Two files: the proposal saved three times today with one copy attached in Mail, the budget saved yesterday. */
function backend() {
  const b = mockBackend({
    recent: [
      item({ itemId: "itm_01", name: LONG, latestSeq: 3, latestAt: at(26, 10, 12) }),
      item({ itemId: "itm_02", name: "Budget_2027.xlsx", latestSeq: 4, latestAt: at(25, 17, 5) }),
    ],
    history: { itm_01: [version({ seq: 3 }), version({ seq: 2 }), version({ seq: 1 })] },
  });
  b.state.handlers.activity = () => ({
    saves: [
      { itemId: "itm_01", versionId: "ver_01", at: at(26, 9, 20) },
      { itemId: "itm_01", versionId: "ver_02", at: at(26, 9, 58) },
      { itemId: "itm_01", versionId: "ver_03", at: at(26, 10, 12) },
    ],
    copies: [handed("itm_01", "ver_02", at(26, 10, 14))],
  });
  b.state.handlers.copies = (a) => (a.item === "itm_01" ? [handed("itm_01", "ver_02", at(26, 10, 14))] : []);
  return b;
}

function captureDrop() {
  let fire: (e: DropEvent) => void = () => {};
  vi.spyOn(api, "onDrop").mockImplementation((cb) => {
    fire = cb;
    return Promise.resolve(() => {});
  });
  return (e: DropEvent) => act(() => fire(e));
}

test("the header counts today's saves and files", async () => {
  backend();
  renderWithClient(<Popover />);
  expect(await screen.findByRole("heading", { name: "Today" })).toBeInTheDocument();
  expect(await screen.findByText("3 saves in 1 file")).toBeInTheDocument();
  expect(screen.getByRole("img", { name: /most at 09:00/ })).toBeInTheDocument();
});

test("a quiet day says no saves yet", async () => {
  const b = backend();
  b.state.handlers.activity = () => ({ saves: [], copies: [] });
  renderWithClient(<Popover />);
  expect(await screen.findByText("No saves yet today")).toBeInTheDocument();
});

test("a row opens in place with its saves and one button", async () => {
  backend();
  renderWithClient(<Popover />);
  const row = await screen.findByRole("button", { name: LONG });
  expect(screen.queryByRole("button", { name: "Open History" })).not.toBeInTheDocument();
  await userEvent.click(row);
  const open = screen.getByRole("region", { name: `${LONG} today` });
  await waitFor(() => expect(within(open).getAllByRole("listitem")).toHaveLength(3));
  expect(within(open).getAllByRole("button")).toHaveLength(1);
  expect(within(open).getByRole("button", { name: "Open History" })).toBeInTheDocument();
  expect(row).toHaveAttribute("aria-expanded", "true");
});

test("only one row is open at a time", async () => {
  backend();
  renderWithClient(<Popover />);
  await userEvent.click(await screen.findByRole("button", { name: LONG }));
  await userEvent.click(screen.getByRole("button", { name: "Budget_2027.xlsx" }));
  expect(screen.queryByRole("region", { name: `${LONG} today` })).not.toBeInTheDocument();
  expect(screen.getByRole("region", { name: "Budget_2027.xlsx today" })).toBeInTheDocument();
});

test("a copy today shows in the sub line and as a filled dot", async () => {
  backend();
  renderWithClient(<Popover />);
  expect(await screen.findByText("3 saves today, Attached in Mail 10:14")).toBeInTheDocument();
  expect(screen.getByText("Last saved Yesterday 17:05")).toBeInTheDocument();
  await userEvent.click(screen.getByRole("button", { name: LONG }));
  const open = screen.getByRole("region", { name: `${LONG} today` });
  await waitFor(() =>
    expect(within(open).getAllByRole("listitem").filter((li) => li.dataset.copied === "true")).toHaveLength(1),
  );
  expect(within(open).getByText("Attached in Mail 10:14")).toBeInTheDocument();
});

test("open history opens that file", async () => {
  const b = backend();
  renderWithClient(<Popover />);
  await userEvent.click(await screen.findByRole("button", { name: LONG }));
  await userEvent.click(screen.getByRole("button", { name: "Open History" }));
  expect(b.called("open_history")[0].args).toEqual({ item: "itm_01" });
});

test("a long turkish name is cut but read out whole", async () => {
  backend();
  renderWithClient(<Popover />);
  const row = await screen.findByRole("button", { name: LONG });
  expect(within(row).getByText("Teklif_Müşteri_2026_…hâli.docx")).toHaveAttribute("title", LONG);
});

test("dropping a tracked file opens its history", async () => {
  const fire = captureDrop();
  const b = backend();
  b.state.handlers.item_at = () => "itm_01";
  renderWithClient(<Popover />);
  await screen.findByRole("heading", { name: "Today" });
  fire({ type: "over" });
  expect(await screen.findByText("Drop a file to see which saved version it is")).toBeInTheDocument();
  fire({ type: "drop", paths: ["/Users/a/Client work/x.docx", "/Users/a/other.docx"] });
  await waitFor(() => expect(b.called("open_history")[0]?.args).toEqual({ item: "itm_01" }));
  expect(b.called("item_at")).toEqual([{ cmd: "item_at", args: { path: "/Users/a/Client work/x.docx" } }]);
  expect(screen.queryByText("Drop a file to see which saved version it is")).not.toBeInTheDocument();
});

test("dropping an unknown file answers which one", async () => {
  const fire = captureDrop();
  const b = backend();
  b.state.handlers.item_at = () => null;
  b.state.handlers.which = () => [
    {
      itemId: "itm_01",
      name: "Teklif.docx",
      version: { ...version({ seq: 2 }), createdAt: at(21, 10, 12), code: "7f3a9c21" },
      copies: [],
    },
  ];
  renderWithClient(<Popover />);
  await screen.findByRole("heading", { name: "Today" });
  fire({ type: "drop", paths: ["/Users/a/Downloads/back.docx"] });
  const card = await screen.findByRole("status");
  expect(card).toHaveTextContent("Teklif.docx, Wednesday 10:12 (7f3a9c21)");
  await userEvent.click(within(card).getByRole("button", { name: "Open History" }));
  expect(b.called("open_history")[0].args).toEqual({ item: "itm_01" });
});

test("dropping an edited copy says so plainly", async () => {
  const fire = captureDrop();
  const b = backend();
  b.state.handlers.item_at = () => null;
  b.state.handlers.which = () => [];
  renderWithClient(<Popover />);
  await screen.findByRole("heading", { name: "Today" });
  fire({ type: "drop", paths: ["/Users/a/Downloads/back.docx"] });
  expect(await screen.findByRole("status")).toHaveTextContent(
    "No saved version has exactly this content. It may have been opened and saved again since it left.",
  );
});

test("dropping a folder is refused plainly", async () => {
  const fire = captureDrop();
  const b = backend();
  b.state.handlers.item_at = () => null;
  b.state.handlers.which = () => {
    throw { code: "not_a_file", title: "That is a folder", hint: "Choose a file." };
  };
  renderWithClient(<Popover />);
  await screen.findByRole("heading", { name: "Today" });
  fire({ type: "drop", paths: ["/Users/a/Client work"] });
  expect(await screen.findByRole("status")).toHaveTextContent("Drop a file, not a folder.");
});

test("no word ready or sent appears", async () => {
  backend();
  const { container } = renderWithClient(<Popover />);
  await screen.findByText("3 saves today, Attached in Mail 10:14");
  await userEvent.click(screen.getByRole("button", { name: LONG }));
  await screen.findByText("Attached in Mail 10:14");
  expect(container.textContent ?? "").not.toMatch(/\bready\b|\bsent\b/i);
});

test("the footer says where saves are kept", async () => {
  backend();
  renderWithClient(<Popover />);
  expect(await screen.findByText("Keeping every save in Client work")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Add Folder…" })).toBeInTheDocument();
});

test("the first launch still invites a folder, and an empty folder still waits", async () => {
  mockBackend({ folders: [], recent: [] });
  const { unmount } = renderWithClient(<Popover />);
  expect(await screen.findByRole("button", { name: "Choose folder" })).toBeInTheDocument();
  unmount();
  mockBackend({ recent: [] });
  renderWithClient(<Popover />);
  expect(await screen.findByText("Save a file in Client work and it shows up here.")).toBeInTheDocument();
});

test("Escape closes the popover", async () => {
  const b = backend();
  renderWithClient(<Popover />);
  await screen.findByRole("heading", { name: "Today" });
  await userEvent.keyboard("{Escape}");
  await waitFor(() => expect(b.called("hide_popover")).toHaveLength(1));
});

test("the popover asks to fit its content once loaded", async () => {
  const observed: Element[] = [];
  vi.stubGlobal(
    "ResizeObserver",
    class {
      constructor(private cb: () => void) {}
      observe(el: Element) {
        observed.push(el);
        this.cb();
      }
      disconnect() {}
    },
  );
  const b = backend();
  renderWithClient(<Popover />);
  await screen.findByRole("heading", { name: "Today" });
  await waitFor(() => expect(b.called("fit_popover").length).toBeGreaterThan(0));
  expect(observed.at(-1)).toHaveAttribute("aria-label", "Folyvo");
  vi.unstubAllGlobals();
});

test("a row reads out its name and its day, and each dot its time", async () => {
  backend();
  renderWithClient(<Popover />);
  const row = await screen.findByRole("button", { name: LONG });
  expect(row).toHaveAccessibleDescription("3 saves today, Attached in Mail 10:14");
  await userEvent.click(row);
  const open = screen.getByRole("region", { name: `${LONG} today` });
  await waitFor(() => expect(within(open).getByRole("listitem", { name: "09:58, a copy left" })).toBeInTheDocument());
  expect(within(open).getByRole("listitem", { name: "10:12" })).toBeInTheDocument();
});
