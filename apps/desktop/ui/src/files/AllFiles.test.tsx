import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { api } from "../api";
import { item, mockBackend, renderWithClient } from "../test-utils";
import { AllFiles } from "./AllFiles";

const at = (h: number) => new Date(2026, 9, 26, h).getTime();

function backend() {
  return mockBackend({
    folders: [
      { path: "/Users/a/Client work", problem: null, reading: null },
      { path: "/Users/a/School", problem: null, reading: null },
    ],
    recent: [
      item({ itemId: "itm_01", name: "Bilal_Resume-3.pdf", path: "/Users/a/Client work/Bilal_Resume-3.pdf", latestAt: at(10) }),
      item({ itemId: "itm_02", name: "Bilal_Resume-2.pdf", path: "/Users/a/Client work/Drafts/Bilal_Resume-2.pdf", latestAt: at(9) }),
      item({ itemId: "itm_03", name: "Müşteri.docx", path: "/Users/a/School/Müşteri.docx", latestAt: at(8), missing: true }),
    ],
  });
}

const options = () => screen.getAllByRole("option").map((o) => within(o).getByTestId("name").textContent);

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(at(11));
});
afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("AllFiles", () => {
  it("asks for every file, not only the latest few", async () => {
    const b = backend();
    renderWithClient(<AllFiles />);
    await screen.findByText("Client work");
    expect(b.called("recent")[0].args.limit).toBeGreaterThan(100_000);
  });

  it("lists every file under its folder, with where it is below that folder", async () => {
    backend();
    renderWithClient(<AllFiles />);
    const work = await screen.findByRole("group", { name: "Client work" });
    expect(within(work).getAllByRole("option")).toHaveLength(2);
    expect(within(work).getByText(/Drafts, last saved/)).toBeInTheDocument();
    expect(within(screen.getByRole("group", { name: "School" })).getAllByRole("option")).toHaveLength(1);
  });

  it("does not name the folder when there is only one", async () => {
    const b = backend();
    b.state.folders = [{ path: "/Users/a/Client work", problem: null, reading: null }];
    b.state.recent = b.state.recent.slice(0, 2);
    renderWithClient(<AllFiles />);
    await screen.findByText("Bilal_Resume-3.pdf");
    expect(screen.queryByRole("group")).not.toBeInTheDocument();
  });

  it("says a file was moved or deleted, and still opens it", async () => {
    const b = backend();
    renderWithClient(<AllFiles />);
    const row = (await screen.findByText("Müşteri.docx")).closest("[role=option]") as HTMLElement;
    expect(row).toHaveTextContent("Moved or deleted");
    await userEvent.click(row);
    expect(b.called("open_history").at(-1)?.args.item).toBe("itm_03");
  });

  it("starts with the cursor in the search and narrows the list as you type", async () => {
    backend();
    renderWithClient(<AllFiles />);
    const search = await screen.findByRole("combobox", { name: "Find a file" });
    expect(search).toHaveFocus();
    await userEvent.keyboard("musteri");
    await waitFor(() => expect(options()).toEqual(["Müşteri.docx"]));
    expect(screen.getByRole("status")).toHaveTextContent("1 file");
  });

  it("opens with the keyboard: arrows move, Enter opens", async () => {
    const b = backend();
    renderWithClient(<AllFiles />);
    const search = await screen.findByRole("combobox", { name: "Find a file" });
    await screen.findByText("Bilal_Resume-3.pdf");
    await userEvent.keyboard("{ArrowDown}{ArrowDown}{ArrowDown}{ArrowUp}");
    const active = document.getElementById(search.getAttribute("aria-activedescendant")!)!;
    expect(active).toHaveTextContent("Bilal_Resume-2.pdf");
    await userEvent.keyboard("{ArrowDown}{Enter}");
    expect(b.called("open_history").at(-1)?.args.item).toBe("itm_03");
  });

  // Browsers now return a promise from scrollIntoView; returned from an effect, it took the page down.
  it("survives moving the cursor when scrolling returns a promise", async () => {
    backend();
    Element.prototype.scrollIntoView = vi.fn(() => Promise.resolve()) as unknown as Element["scrollIntoView"];
    try {
      renderWithClient(<AllFiles />);
      await screen.findByText("Bilal_Resume-3.pdf");
      await userEvent.keyboard("{ArrowDown}{ArrowDown}");
      expect(screen.getByRole("listbox")).toBeInTheDocument();
    } finally {
      delete (Element.prototype as Partial<Element>).scrollIntoView;
    }
  });

  it("Enter opens the first match of a search", async () => {
    const b = backend();
    renderWithClient(<AllFiles />);
    await screen.findByText("Bilal_Resume-3.pdf");
    await userEvent.keyboard("resume-2{Enter}");
    expect(b.called("open_history").at(-1)?.args.item).toBe("itm_02");
  });

  it("Escape clears the search first, then closes the window", async () => {
    backend();
    const close = vi.spyOn(api, "closeWindow").mockResolvedValue();
    renderWithClient(<AllFiles />);
    const search = await screen.findByRole("combobox", { name: "Find a file" });
    await userEvent.keyboard("zzz{Escape}");
    expect(search).toHaveValue("");
    expect(close).not.toHaveBeenCalled();
    await userEvent.keyboard("{Escape}");
    expect(close).toHaveBeenCalledOnce();
  });

  it("says what it looked for and offers a way forward when nothing matches", async () => {
    const b = backend();
    b.state.picked = "/Users/a/New";
    renderWithClient(<AllFiles />);
    await screen.findByText("Bilal_Resume-3.pdf");
    await userEvent.keyboard("zzz");
    expect(await screen.findByText("No files named “zzz”")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Add Folder…" }));
    await waitFor(() => expect(b.called("add_folder")[0]?.args.path).toBe("/Users/a/New"));
  });

  it("clears a search that found nothing, before anything else", async () => {
    backend();
    renderWithClient(<AllFiles />);
    await screen.findByText("Bilal_Resume-3.pdf");
    await userEvent.keyboard("zzz");
    await userEvent.click(await screen.findByRole("button", { name: "Clear Search" }));
    expect(await screen.findByText("Bilal_Resume-3.pdf")).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "Find a file" })).toHaveFocus();
  });

  it("keeps the keyboard working after a click on a row or the list", async () => {
    vi.spyOn(api, "closeWindow").mockResolvedValue();
    backend();
    renderWithClient(<AllFiles />);
    const search = await screen.findByRole("combobox", { name: "Find a file" });
    await userEvent.click(await screen.findByText("Bilal_Resume-3.pdf"));
    expect(search).toHaveFocus();
    await userEvent.keyboard("zzz");
    expect(search).toHaveValue("zzz");
  });

  it("does not open a file with the Enter that ends typing in an input method", async () => {
    const b = backend();
    renderWithClient(<AllFiles />);
    const search = await screen.findByRole("combobox", { name: "Find a file" });
    await screen.findByText("Bilal_Resume-3.pdf");
    fireEvent.keyDown(search, { key: "Enter", isComposing: true });
    expect(b.called("open_history")).toHaveLength(0);
  });

  it("keeps the end of a long name, where similar names differ", async () => {
    const b = backend();
    const long = `${"Teklif_Müşteri_2026_son_hâli_".repeat(3)}v7.docx`;
    b.state.recent = [item({ name: long, path: `/Users/a/Client work/${long}` })];
    renderWithClient(<AllFiles />);
    const name = await screen.findByTestId("name");
    expect(name.textContent).toMatch(/…[^…]*v7\.docx$/);
    expect(name).toHaveAttribute("title", long);
  });

  it("offers a folder when nothing is kept yet", async () => {
    const b = backend();
    b.state.recent = [];
    b.state.folders = [];
    renderWithClient(<AllFiles />);
    expect(await screen.findByText("No files kept yet")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add Folder…" })).toBeInTheDocument();
  });
});
