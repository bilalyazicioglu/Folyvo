import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { FolderList } from "./FolderList";

const ok = { path: "/Users/a/Client work", problem: null };
const broken = {
  path: "/Volumes/Backup/Archive",
  problem: { code: "cannot_watch", title: "Folyvo can't watch that folder", hint: "Check that the folder still exists, then choose it again." },
};

test("every folder can be removed and the button names it", async () => {
  const onRemove = vi.fn();
  render(<FolderList folders={[ok, broken]} onAdd={vi.fn()} onRemove={onRemove} busy={false} defaultOpen />);
  await userEvent.click(screen.getByRole("button", { name: "Stop watching Client work" }));
  expect(onRemove).toHaveBeenCalledWith("/Users/a/Client work");
  expect(screen.getByRole("button", { name: "Stop watching Archive" })).toBeInTheDocument();
});

// Review Focus 2: a folder that is gone is listed with a plain message, the others stay usable.
test("a folder with a problem shows what happened and what to do", () => {
  render(<FolderList folders={[ok, broken]} onAdd={vi.fn()} onRemove={vi.fn()} busy={false} defaultOpen />);
  expect(screen.getByText("Folyvo can't watch that folder")).toBeInTheDocument();
  expect(screen.getByText("Check that the folder still exists, then choose it again.")).toBeInTheDocument();
});

test("a new folder is added with its own button", async () => {
  const onAdd = vi.fn();
  render(<FolderList folders={[ok]} onAdd={onAdd} onRemove={vi.fn()} busy={false} defaultOpen />);
  await userEvent.click(screen.getByRole("button", { name: "Add folder" }));
  expect(onAdd).toHaveBeenCalledOnce();
});

test("the section summary counts the folders", () => {
  render(<FolderList folders={[ok, broken]} onAdd={vi.fn()} onRemove={vi.fn()} busy={false} />);
  expect(screen.getByText("Watched folders (2)")).toBeInTheDocument();
});
