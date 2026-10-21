import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { NoFolders, WaitingForSave } from "./EmptyState";

// Review Focus 5: the first launch is an invitation, not an empty list.
test("the first launch invites the person to choose a folder", async () => {
  const onChoose = vi.fn();
  render(<NoFolders onChoose={onChoose} busy={false} />);
  expect(screen.getByRole("heading", { name: "Choose a folder to watch" })).toBeInTheDocument();
  expect(screen.getByText(/Everything stays on this Mac/)).toBeInTheDocument();
  await userEvent.click(screen.getByRole("button", { name: "Choose folder" }));
  expect(onChoose).toHaveBeenCalledOnce();
});

test("with a folder but nothing saved yet it says what to do", () => {
  render(<WaitingForSave folder="Client work" />);
  expect(screen.getByRole("heading", { name: "Waiting for the first save" })).toBeInTheDocument();
  expect(screen.getByText("Save a file in Client work and it shows up here.")).toBeInTheDocument();
});
