<p align="center">
  <a href="https://folyvo.com"><img src="docs/assets/folyvo-icon.svg" alt="Folyvo" width="96"></a>
</p>

<h3 align="center">Every save, kept. Nothing to learn.</h3>

<p align="center">
  Folyvo keeps saved versions of your files on your Mac. Keep using Word.
</p>

<p align="center">
  <a href="https://folyvo.com">folyvo.com</a>
</p>

<p align="center">
  <a href="https://github.com/bilalyazicioglu/Folyvo/actions/workflows/ci.yml"><img src="https://github.com/bilalyazicioglu/Folyvo/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg" alt="License: GPL-3.0-or-later"></a>
</p>

<p align="center">
  <picture>
    <source srcset="https://raw.githubusercontent.com/bilalyazicioglu/Folyvo/main/docs/assets/folyvo-demo.avif" type="image/avif">
    <img src="docs/assets/folyvo-demo.gif" alt="Folyvo opened from the menu bar: a document's saved times, an earlier save with one paragraph removed, and Restore, which saves the current file first so nothing is lost" width="720" height="450">
  </picture>
</p>

---

You shorten a proposal, change a few numbers, save over a paragraph you meant to keep. Usually the old file is gone behind the new one. With Folyvo running, your earlier saves are still there.

Choose a folder. Then work in Word, Excel, or any app you already use. There is nothing to set up for each file and no new words to learn.

## What it does

- **Keeps your saves.** Each time a file in your folder is saved, Folyvo keeps that version. Open it from the menu bar or from Finder's right-click menu.
- **Stays small.** A file that never changed takes no extra room, and the menu bar shows how much Folyvo uses. Every save from the last 14 days stays, then one a day; saves you mark, send or write a note on stay for good. Folyvo asks before it first lets any go, and you can keep every save instead.
- **Shows what changed.** Under each saved time, a line says what changed since the save before: "2 paragraphs changed", "3 cells changed", "Page 2 changed". One click shows the page with the changes marked. Works for Word, Excel, PDF and plain text.
- **Takes you back.** See your file as it was at any saved time. Restore it, or open a separate copy in Word and keep going.
- **Remembers which copy left your Mac.** Share a saved version through Mail, AirDrop or Messages, and Folyvo notes it next to that version: "Attached in Mail 10:14".
- **Knows a file when it comes back.** Right-click any file in Finder › Which One Is This?, even one with a different name, and Folyvo tells you which saved version it is. If the contents match none, it says so instead of guessing.

## On your Mac. And only your Mac.

No account. No cloud. Folyvo reads your files and keeps copies in your own Library folder. Nothing is uploaded, and nothing leaves your computer unless you send it.

The one thing Folyvo asks the internet is, once a day, whether a newer Folyvo exists. The request goes to GitHub and carries nothing about you or your files; GitHub sees it the way it sees any visit to a page. Nothing is installed until you choose to, and you can turn the check off in the window that shows how much room Folyvo takes.

## Status

Folyvo is in pre-beta. [Download it from Releases](https://github.com/bilalyazicioglu/Folyvo/releases/latest) for macOS 14 or later, on Apple silicon and Intel Macs, or [run it from source](docs/development/desktop.md).

Windows is not planned yet. If you want it, add a 👍 to [#67](https://github.com/bilalyazicioglu/Folyvo/issues/67).

## For developers

Folyvo gives people who work with documents the guarantees developers get from Git, without the vocabulary: no commits, no branches, no staging.

It is a Rust workspace with a Tauri 2 and React app on top:

- `crates/folyvo-core` keeps the saves: a content-addressed store and SQLite, watching folders and reconciling on launch.
- `crates/folyvo-compare` reads Word, Excel and text files and compares two of them, with limits on size, depth and time so a hostile file cannot hang or fill the disk.
- `crates/folyvo-cli` is a small command line for the same store.
- `apps/desktop` is the menu bar app, its Finder extension and Services.

See the [documentation](docs/) and [how to run the app](docs/development/desktop.md).

## Contributing

Contributions are welcome. Please read [CONTRIBUTING.md](CONTRIBUTING.md) first, and note the [code of conduct](CODE_OF_CONDUCT.md). Issues marked [good first issue](https://github.com/bilalyazicioglu/Folyvo/labels/good%20first%20issue) or [help wanted](https://github.com/bilalyazicioglu/Folyvo/labels/help%20wanted) are a good place to start. Report security problems privately, as described in [SECURITY.md](SECURITY.md).

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
