import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { NoFolders, ReadingFolders, WaitingForSave } from "./EmptyState";

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

const reading = (done: number, total: number, secondsLeft: number | null) => ({
  path: "/Users/a/Documents",
  problem: null,
  reading: { done, total, secondsLeft, stopping: false },
});

test("reading shows how many files are done and about how long is left", () => {
  render(<ReadingFolders folders={[reading(8986, 17233, 120)]} onStop={() => {}} />);
  expect(screen.getByRole("progressbar", { name: "Reading your files" })).toHaveAttribute("value", "8986");
  expect(screen.getByText("8,986 of 17,233 files")).toBeInTheDocument();
  expect(screen.getByText("About 2 minutes left")).toBeInTheDocument();
});

test("the last minute and an unknown time are said plainly", () => {
  const { rerender } = render(<ReadingFolders folders={[reading(10, 20, 30)]} onStop={() => {}} />);
  expect(screen.getByText("Less than a minute left")).toBeInTheDocument();
  rerender(<ReadingFolders folders={[reading(10, 20, null)]} onStop={() => {}} />);
  expect(screen.queryByText(/left/)).not.toBeInTheDocument();
});

test("before the files are counted there is no count to show", () => {
  render(<ReadingFolders folders={[reading(0, 0, null)]} onStop={() => {}} />);
  expect(screen.getByRole("progressbar", { name: "Reading your files" })).not.toHaveAttribute("value");
  expect(screen.queryByText(/of 0 files/)).not.toBeInTheDocument();
});

test("several folders add up, and the slowest says how long is left", () => {
  render(<ReadingFolders folders={[reading(10, 100, 60), reading(5, 50, 300)]} onStop={() => {}} />);
  expect(screen.getByText("15 of 150 files")).toBeInTheDocument();
  expect(screen.getByText("About 5 minutes left")).toBeInTheDocument();
});

test("a read can be stopped from where it shows", async () => {
  const onStop = vi.fn();
  render(<ReadingFolders folders={[reading(10, 100, null)]} onStop={onStop} />);
  await userEvent.click(screen.getByRole("button", { name: "Stop Reading" }));
  expect(onStop).toHaveBeenCalledWith(reading(10, 100, null).path);
});

test("a read that was asked to stop says so and cannot be asked twice", () => {
  const folder = reading(10, 100, null);
  const stopping = { ...folder, reading: { ...folder.reading!, stopping: true } };
  render(<ReadingFolders folders={[stopping]} onStop={vi.fn()} />);
  expect(screen.getByText("Stopping…")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Stop Reading" })).toBeNull();
});
