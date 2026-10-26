import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { api } from "../api";
import { item, mockBackend, renderWithClient, version } from "../test-utils";
import { Versions } from "./Versions";

const at = (d: number, h: number, m: number, s = 0) => new Date(2026, 9, d, h, m, s).getTime();
const NOW = at(26, 10, 42);
const NAME = "Teklif_Müşteri_2026_son_hâli.docx";

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(NOW);
});
afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

/** Three saves: today 10:12 (newest), today 09:58 (attached in Mail), yesterday 17:20. */
function backend() {
  const b = mockBackend({
    recent: [item({ itemId: "itm_01", name: NAME, latestSeq: 3, latestAt: at(26, 10, 12) })],
    history: {
      itm_01: [
        version({ seq: 3, createdAt: at(26, 10, 12), size: 48_000 }),
        version({ seq: 2, createdAt: at(26, 9, 58) }),
        version({ seq: 1, createdAt: at(25, 17, 20), note: "Before the call" }),
      ],
    },
  });
  b.state.handlers.preview = () => ({ picture: "data:image/png;base64,AAAA", appName: "Microsoft Word" });
  b.state.handlers.copies = () => [
    { itemId: "itm_01", versionId: "ver_02", kind: "handed", service: "mail", targetPath: null, at: at(26, 10, 14) },
  ];
  b.state.handlers.export = (a) => ({ destination: `/Users/a/Desktop/${NAME}`, versionId: a.version });
  b.state.handlers.restore = () => ({ versionId: "ver_04", seq: 4 });
  return b;
}

const option = (time: string) => screen.findByRole("option", { name: new RegExp(`^${time}`) });

test("the picture is an image, never text", async () => {
  const { container } = (backend(), renderWithClient(<Versions itemId="itm_01" />));
  const img = await screen.findByRole("img", { name: `Picture of ${NAME} as it was Today 10:12` });
  expect(img).toHaveAttribute("src", "data:image/png;base64,AAAA");
  expect(container.querySelector("textarea, input, [contenteditable]")).toBeNull();
});

test("no picture shows the kind and says so", async () => {
  const b = backend();
  b.state.handlers.preview = () => ({ picture: null, appName: null });
  renderWithClient(<Versions itemId="itm_01" />);
  expect(await screen.findByText("No preview for this kind of file.")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Open" })).toBeInTheDocument();
});

test("the newest offers open in app and no restore", async () => {
  const b = backend();
  renderWithClient(<Versions itemId="itm_01" />);
  const open = await screen.findByRole("button", { name: "Open in Microsoft Word" });
  expect(screen.queryByRole("button", { name: "Restore" })).not.toBeInTheDocument();
  expect(screen.getByText("Today 10:12, 47 KB. This is the file as it is now.")).toBeInTheDocument();
  await userEvent.click(open);
  expect(b.called("open_version")[0].args).toEqual({ item: "itm_01", version: "ver_03", newest: true });
});

test("an older time offers open a copy and restore", async () => {
  const b = backend();
  renderWithClient(<Versions itemId="itm_01" />);
  await userEvent.click(await option("17:20"));
  await userEvent.click(await screen.findByRole("button", { name: "Open a Copy in Microsoft Word" }));
  expect(b.called("open_version")[0].args).toEqual({ item: "itm_01", version: "ver_01", newest: false });
  expect(screen.getByRole("button", { name: "Restore" })).toBeInTheDocument();
});

test("restore asks first and runs only on confirm", async () => {
  const b = backend();
  renderWithClient(<Versions itemId="itm_01" />);
  await userEvent.click(await option("17:20"));
  await userEvent.click(screen.getByRole("button", { name: "Restore" }));
  expect(b.called("restore")).toHaveLength(0);
  const confirm = screen.getByRole("group", { name: "Confirm restore" });
  expect(confirm).toHaveTextContent("Restore Yesterday 17:20? Folyvo saves the current file first, so nothing is lost.");
  await userEvent.click(within(confirm).getByRole("button", { name: "Cancel" }));
  expect(b.called("restore")).toHaveLength(0);
  await userEvent.click(screen.getByRole("button", { name: "Restore" }));
  await userEvent.click(within(screen.getByRole("group", { name: "Confirm restore" })).getByRole("button", { name: "Restore" }));
  await waitFor(() => expect(b.called("restore")).toHaveLength(1));
  expect(b.called("restore")[0].args).toEqual({ item: "itm_01", version: "ver_01" });
});

test("save a copy asks for a folder and offers finder", async () => {
  const b = backend();
  b.state.picked = "/Users/a/Desktop";
  renderWithClient(<Versions itemId="itm_01" />);
  await userEvent.click(await screen.findByRole("button", { name: "Save a Copy…" }));
  const status = await screen.findByRole("status");
  expect(status).toHaveTextContent("Saved a copy to Desktop");
  expect(b.called("export")[0].args).toEqual({ item: "itm_01", version: "ver_03", destDir: "/Users/a/Desktop" });
  await userEvent.click(within(status).getByRole("button", { name: "Show in Finder" }));
  expect(b.called("reveal")[0].args).toEqual({ path: `/Users/a/Desktop/${NAME}` });
});

test("copies show as chips on their time", async () => {
  backend();
  renderWithClient(<Versions itemId="itm_01" />);
  const row = await option("09:58");
  await waitFor(() => expect(within(row).getByText("Attached in Mail")).toBeInTheDocument());
  await userEvent.click(row);
  expect(screen.getByText("Today 09:58, Attached in Mail")).toBeInTheDocument();
  expect(within(await option("17:20")).getByText("Before the call")).toBeInTheDocument();
});

test("two saves in one minute show seconds", async () => {
  const b = backend();
  b.state.history.itm_01 = [
    version({ seq: 2, createdAt: at(26, 10, 12, 40) }),
    version({ seq: 1, createdAt: at(26, 10, 12, 5) }),
  ];
  renderWithClient(<Versions itemId="itm_01" />);
  expect(await option("10:12:40")).toBeInTheDocument();
  expect(await option("10:12:05")).toBeInTheDocument();
});

test("arrow keys move through times", async () => {
  backend();
  renderWithClient(<Versions itemId="itm_01" />);
  const first = await option("10:12");
  expect(first).toHaveAttribute("aria-selected", "true");
  first.focus();
  await userEvent.keyboard("{ArrowDown}");
  expect(await option("09:58")).toHaveAttribute("aria-selected", "true");
  await userEvent.keyboard("{ArrowDown}{ArrowDown}");
  expect(await option("17:20")).toHaveAttribute("aria-selected", "true");
  await userEvent.keyboard("{ArrowUp}");
  expect(await option("09:58")).toHaveAttribute("aria-selected", "true");
});

test("the window title is the whole file name", async () => {
  backend();
  renderWithClient(<Versions itemId="itm_01" />);
  expect(await screen.findByRole("heading", { name: NAME })).toBeInTheDocument();
  await waitFor(() => expect(document.title).toBe(NAME));
});

test("no word ready or sent or export appears", async () => {
  backend();
  const { container } = renderWithClient(<Versions itemId="itm_01" />);
  await option("09:58");
  await screen.findByText("Attached in Mail");
  expect(container.textContent ?? "").not.toMatch(/\bready\b|\bsent\b|export/i);
});

test("done closes the window", async () => {
  const b = backend();
  renderWithClient(<Versions itemId="itm_01" />);
  await userEvent.click(await screen.findByRole("button", { name: "Done" }));
  await waitFor(() => expect(b.called("plugin:window|close")).toHaveLength(1));
});

test("a restore is confirmed by a notification", async () => {
  const notify = vi.spyOn(api, "notify").mockResolvedValue();
  backend();
  renderWithClient(<Versions itemId="itm_01" />);
  await userEvent.click(await option("17:20"));
  await userEvent.click(screen.getByRole("button", { name: "Restore" }));
  await userEvent.click(within(screen.getByRole("group", { name: "Confirm restore" })).getByRole("button", { name: "Restore" }));
  await waitFor(() => expect(notify).toHaveBeenCalledTimes(1));
  expect(notify).toHaveBeenCalledWith("Back to Yesterday 17:20", `${NAME}. Today's version is still in its history.`);
});

test("a failed restore sends no notification", async () => {
  const notify = vi.spyOn(api, "notify").mockResolvedValue();
  const b = backend();
  b.state.handlers.restore = () => {
    throw { code: "file_unavailable", title: "Folyvo cannot reach that file", hint: "Check it and try again." };
  };
  renderWithClient(<Versions itemId="itm_01" />);
  await userEvent.click(await option("17:20"));
  await userEvent.click(screen.getByRole("button", { name: "Restore" }));
  await userEvent.click(within(screen.getByRole("group", { name: "Confirm restore" })).getByRole("button", { name: "Restore" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Folyvo cannot reach that file");
  expect(notify).not.toHaveBeenCalled();
});
