# The desktop app

Folyvo's desktop app is a menu bar popover and a window with the saved versions of one file. It runs on macOS only for now. The core library underneath it is platform independent and is tested on macOS, Windows and Linux.

## Prerequisites

- Rust (stable) and the Xcode command line tools
- Node 22 or newer
- The Tauri command line tool: `cargo install tauri-cli --version "^2.12" --locked`

## Run it

```sh
npm --prefix apps/desktop/ui ci
cd apps/desktop/src-tauri
FOLYVO_VAULT=/tmp/folyvo-dev cargo tauri dev
```

Without `FOLYVO_VAULT` the vault is `~/Library/Application Support/Folyvo`. The app has no Dock icon: look for the layered glyph in the menu bar.

To see the screens without the app, run `npm --prefix apps/desktop/ui run dev` and open `http://localhost:1420/?demo=1` (the popover) or `http://localhost:1420/?demo=1&view=versions&item=itm_demo1` (the versions window). Demo mode serves fixture data from memory.

## Test it

```sh
cargo test -p folyvo-desktop            # also regenerates the TypeScript types: commit the result
npm --prefix apps/desktop/ui test
npm --prefix apps/desktop/ui run build
```

CI fails when the generated files in `apps/desktop/ui/src/generated` differ from what `cargo test -p folyvo-desktop` produces.

## How the pieces fit

| Piece | What it does |
|---|---|
| `crates/folyvo-core/src/runner.rs` | Watches one folder and turns saves into versions. The command line tool and the app both use it. |
| `src-tauri/src/dto.rs` | The data types the UI receives. They generate their own TypeScript. |
| `src-tauri/src/errors.rs` | Turns core errors into a title and a hint in plain English. |
| `src-tauri/src/api.rs` | Everything the UI can ask for, as plain functions over the engine. |
| `src-tauri/src/runtime.rs` | Owns the engine, one runner per watched folder, and the stream of events to the UI. |
| `src-tauri/src/toggle.rs` | Decides what a click on the menu bar icon means, so the click that closes the popover does not reopen it. |
| `src-tauri/src/preview.rs` | Draws a picture of a saved version with Quick Look and finds the app that opens it. |
| `src-tauri/src/fit.rs` | The popover's size: fixed width, height that follows its content. |
| `src-tauri/src/commands.rs`, `tray.rs` | The thin layer that wires the above to Tauri: commands, tray icon, the glass popover and the versions window. |
| `ui/` | The React screens. |

## First launch

Contributor builds are not signed. To open one, right click the app and choose Open, or open System Settings, Privacy & Security and press **Open Anyway**. For a build you made yourself, `xattr -dr com.apple.quarantine Folyvo.app` removes the quarantine flag. macOS asks once per folder (Desktop, Documents, Downloads) whether Folyvo may access it. That prompt is part of the flow.

## Manual demo

Walk through this on a real Mac whenever the window code changes:

1. Start the app. The layered glyph appears in the menu bar and there is no Dock icon.
2. Click it. The popover opens under the icon on a glass background and says "Choose a folder to watch".
3. Choose a test folder. The body says "Waiting for the first save" and the footer "Keeping every save in" and the folder name.
4. Save a text file in that folder three times, a minute or so apart. Within about five seconds of each save the "Today" header counts it, the current hour's bar grows, and the file's row says "3 saves today".
5. Click the row. It opens in place with three dots on a line from the first hour to "now", and one button, **Open History**. Click another row: the first one closes.
6. Click **Open History**. The versions window shows a picture of the file, never an editable page. The times are on the right, grouped by day; arrow keys move through them.
7. Pick an older time. The top button reads "Open a Copy in" and the app name. Press **Save a Copy…** and choose a folder. The notice says "Saved a copy to" and the folder; **Show in Finder** reveals it.
8. Drag that copy onto the popover. While it hovers the popover says "Drop a file to see which saved version it is"; on drop it names the file, the time and the version code.
9. Add a line to the copy, save it and drop it again. The popover says no saved version has exactly this content.
10. In the versions window pick an older time and press **Restore**. Nothing happens until you confirm. After confirming, the file on disk has the old content, a new time is on top, and a notification says "Back to" and the time.
11. In a Terminal at the repository root, run `FOLYVO_VAULT=/tmp/folyvo-dev cargo run -q -p folyvo-cli -- which <the copy from step 7>`. It names the same version as the popover did. The app and the command line may have the vault open at the same time.
12. Turn on System Settings, Accessibility, Display, Reduce Transparency. The popover becomes solid and stays readable; turn it off again. Do the same in Dark Mode.
13. Click the menu bar icon while the popover is open: it closes and does not reopen. Press Escape: it closes.
14. Quit and start the app again. The folder is still watched, and a file saved while the app was closed appears as a new time.
15. Remove the folder under "Watched folders". Watching stops and the history stays.
