import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { mockBackend, renderWithClient } from "../test-utils";
import { FirstWindow } from "./FirstWindow";

function backend() {
  const b = mockBackend({ folders: [] });
  b.state.handlers.suggested_folders = () => ["/Users/a/Documents", "/Users/a/Desktop"];
  return b;
}
const keep = () => screen.getByRole("button", { name: "Keep These Folders" });

describe("FirstWindow", () => {
  it("says what Folyvo does and where to find it", async () => {
    backend();
    renderWithClient(<FirstWindow />);
    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("Every save, kept.");
    expect(screen.getByText(/keeps every saved version of the files in the folders you choose/)).toBeInTheDocument();
    expect(screen.getByText(/stays in the menu bar/)).toBeInTheDocument();
    expect(screen.getByText(/right-click menu/)).toBeInTheDocument();
    await screen.findByRole("checkbox", { name: "Documents" });
  });

  it("suggests folders without choosing them: each one is copied as it is saved", async () => {
    backend();
    renderWithClient(<FirstWindow />);
    expect(await screen.findByRole("checkbox", { name: "Documents" })).not.toBeChecked();
    expect(screen.getByRole("checkbox", { name: "Desktop" })).not.toBeChecked();
    expect(keep()).toBeDisabled();
  });

  it("keeps the chosen folders, then hands over to the menu bar", async () => {
    const b = backend();
    renderWithClient(<FirstWindow />);
    await userEvent.click(await screen.findByRole("checkbox", { name: "Desktop" }));
    await userEvent.click(keep());
    await waitFor(() => expect(b.called("finish_first_window")).toHaveLength(1));
    expect(b.called("add_folder").map((c) => c.args.path)).toEqual(["/Users/a/Desktop"]);
  });

  it("adds another folder as a chosen row", async () => {
    const b = backend();
    b.state.picked = "/Users/a/Client work";
    renderWithClient(<FirstWindow />);
    await screen.findByRole("checkbox", { name: "Documents" });
    await userEvent.click(screen.getByRole("button", { name: "Choose Another Folder…" }));
    expect(await screen.findByRole("checkbox", { name: "Client work" })).toBeChecked();
    expect(keep()).toBeEnabled();
    await userEvent.click(screen.getByRole("button", { name: "Choose Another Folder…" }));
    expect(screen.getAllByRole("checkbox", { name: "Client work" })).toHaveLength(1);
  });

  it("stays open and says why when a folder cannot be kept", async () => {
    const b = backend();
    b.state.handlers.add_folder = () => {
      throw { code: "folder_not_allowed", title: "Folyvo cannot watch this folder", hint: "Choose another." };
    };
    renderWithClient(<FirstWindow />);
    await userEvent.click(await screen.findByRole("checkbox", { name: "Documents" }));
    await userEvent.click(keep());
    expect(await screen.findByRole("alert")).toHaveTextContent("Folyvo cannot watch this folder");
    expect(b.called("finish_first_window")).toHaveLength(0);
  });

  it("can be left without choosing", async () => {
    const b = backend();
    renderWithClient(<FirstWindow />);
    await userEvent.click(screen.getByRole("button", { name: "Not Now" }));
    await waitFor(() => expect(b.called("finish_first_window")).toHaveLength(1));
    expect(b.called("add_folder")).toHaveLength(0);
  });
});
