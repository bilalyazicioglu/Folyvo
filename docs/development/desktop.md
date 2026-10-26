# The desktop app

Folyvo's desktop app is a menu bar popover and a floating history panel. It runs on macOS only for now. The core library underneath it is platform independent and is tested on macOS, Windows and Linux.

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

To see the screens without the app, run `npm --prefix apps/desktop/ui run dev` and open `http://localhost:1420/?demo=1` (the popover) or `http://localhost:1420/?demo=1&view=panel&item=itm_demo1` (the history panel). Demo mode serves fixture data from memory.

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
| `src-tauri/src/commands.rs`, `tray.rs` | The thin layer that wires the above to Tauri: commands, tray icon, popover and panel windows. |
| `ui/` | The React screens. |

## First launch

Contributor builds are not signed. To open one, right click the app and choose Open, or open System Settings, Privacy & Security and press **Open Anyway**. For a build you made yourself, `xattr -dr com.apple.quarantine Folyvo.app` removes the quarantine flag. macOS asks once per folder (Desktop, Documents, Downloads) whether Folyvo may access it. That prompt is part of the flow.

## Manual demo

Walk through this on a real Mac whenever the window code changes:

1. Start the app. The layered glyph appears in the menu bar and there is no Dock icon.
2. Click it. The popover opens under the icon and says "Choose a folder to watch".
3. Choose a test folder. The header reads "Watching" and the folder name, and the body says "Waiting for the first save".
4. Save a text file in that folder. Within about five seconds a card with one layer appears.
5. Change and save it twice. Layers stack with the newest on top and the card offers "Export v3".
6. Click "All versions" and press "Export v1" on the oldest row. Choose a folder. The notice says "Exported v1". "Show in Finder" reveals a copy that opens normally and can be edited.
7. In Terminal, run `folyvo which <that copy>`. It names the file, the time of v1 and its version code.
8. Add a line to the copy, save it and run `folyvo which` again. It says no saved version has exactly this content.
9. In the panel, restore v1 and confirm. The file on disk has the old content and a new layer called "Restored" is on top.
10. Click the menu bar icon while the popover is open: it closes and does not reopen. Press Escape: it closes.
11. Quit and start the app again. The folder is still watched, and a file saved while the app was closed appears as a new version.
12. Remove the folder under "Watched folders". Watching stops and the history stays.
