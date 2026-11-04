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
  reading: { done, total, secondsLeft, state: "reading" as const, quiet: false },
});

test("reading shows how many files are done and about how long is left", () => {
  render(<ReadingFolders folders={[reading(8986, 17233, 120)]} onPause={() => {}} onResume={() => {}} onStop={() => {}} />);
  expect(screen.getByRole("progressbar", { name: "Reading your files" })).toHaveAttribute("value", "8986");
  expect(screen.getByText("8,986 of 17,233 files")).toBeInTheDocument();
  expect(screen.getByText("About 2 minutes left")).toBeInTheDocument();
});

test("the last minute and an unknown time are said plainly", () => {
  const { rerender } = render(<ReadingFolders folders={[reading(10, 20, 30)]} onPause={() => {}} onResume={() => {}} onStop={() => {}} />);
  expect(screen.getByText("Less than a minute left")).toBeInTheDocument();
  rerender(<ReadingFolders folders={[reading(10, 20, null)]} onPause={() => {}} onResume={() => {}} onStop={() => {}} />);
  expect(screen.queryByText(/left/)).not.toBeInTheDocument();
});

test("before the files are counted there is no count to show", () => {
  render(<ReadingFolders folders={[reading(0, 0, null)]} onPause={() => {}} onResume={() => {}} onStop={() => {}} />);
  expect(screen.getByRole("progressbar", { name: "Reading your files" })).not.toHaveAttribute("value");
  expect(screen.queryByText(/of 0 files/)).not.toBeInTheDocument();
});

test("several folders add up, and the slowest says how long is left", () => {
  render(<ReadingFolders folders={[reading(10, 100, 60), reading(5, 50, 300)]} onPause={() => {}} onResume={() => {}} onStop={() => {}} />);
  expect(screen.getByText("15 of 150 files")).toBeInTheDocument();
  expect(screen.getByText("About 5 minutes left")).toBeInTheDocument();
});

const inState = (state: "pausing" | "paused" | "stopping", done = 10, total = 100) => {
  const folder = reading(done, total, null);
  return { ...folder, reading: { ...folder.reading, state } };
};

test("a read can be paused from where it shows", async () => {
  const onPause = vi.fn();
  render(<ReadingFolders folders={[reading(10, 100, null)]} onPause={onPause} onResume={vi.fn()} onStop={vi.fn()} />);
  await userEvent.click(screen.getByRole("button", { name: "Pause Reading" }));
  expect(onPause).toHaveBeenCalledWith(reading(10, 100, null).path);
});

test("a read asked to pause says so and cannot be asked twice", () => {
  render(<ReadingFolders folders={[inState("pausing")]} onPause={vi.fn()} onResume={vi.fn()} onStop={vi.fn()} />);
  expect(screen.getByText("Pausing…")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Pause Reading" })).toBeNull();
});

test("a folder on its way out offers nothing", () => {
  render(<ReadingFolders folders={[inState("stopping")]} onPause={vi.fn()} onResume={vi.fn()} onStop={vi.fn()} />);
  expect(screen.getByText("Stopping…")).toBeInTheDocument();
  expect(screen.queryByRole("button")).toBeNull();
});

test("a paused read says how far it got, can resume, and says it will not wait for ever", async () => {
  const onResume = vi.fn();
  render(<ReadingFolders folders={[inState("paused", 1182, 3967)]} onPause={vi.fn()} onResume={onResume} onStop={vi.fn()} />);
  expect(screen.getByRole("heading", { name: "Reading paused" })).toBeInTheDocument();
  expect(screen.getByText("1,182 of 3,967 files")).toBeInTheDocument();
  expect(screen.getByText(/kept for a day/)).toBeInTheDocument();
  await userEvent.click(screen.getByRole("button", { name: "Resume Reading" }));
  expect(onResume).toHaveBeenCalledWith(reading(10, 100, null).path);
});

test("a paused read can be stopped", async () => {
  const onStop = vi.fn();
  render(<ReadingFolders folders={[inState("paused")]} onPause={vi.fn()} onResume={vi.fn()} onStop={onStop} />);
  await userEvent.click(screen.getByRole("button", { name: "Stop Reading" }));
  expect(onStop).toHaveBeenCalledWith(reading(10, 100, null).path);
});

test("a read paused before Folyvo closed shows no empty bar", () => {
  render(<ReadingFolders folders={[inState("paused", 0, 0)]} onPause={vi.fn()} onResume={vi.fn()} onStop={vi.fn()} />);
  expect(screen.queryByRole("progressbar")).toBeNull();
  expect(screen.getByRole("button", { name: "Resume Reading" })).toBeInTheDocument();
});

test("a folder still being read pauses before a paused one offers to resume", async () => {
  const onPause = vi.fn();
  const other = { ...inState("paused"), path: "/Users/a/Other" };
  render(<ReadingFolders folders={[reading(10, 100, null), other]} onPause={onPause} onResume={vi.fn()} onStop={vi.fn()} />);
  await userEvent.click(screen.getByRole("button", { name: "Pause Reading" }));
  expect(onPause).toHaveBeenCalledTimes(1);
  expect(onPause).toHaveBeenCalledWith("/Users/a/Documents");
});
