# Contributing to Folyvo

Thank you for taking the time. This page explains how changes get into the project.

## Before you start

- For anything larger than a small fix, open an issue or a discussion first so we can agree on the direction before you write code.
- Be kind. Everyone taking part is expected to follow the [code of conduct](CODE_OF_CONDUCT.md).
- Security problems are not reported in public issues. See [SECURITY.md](SECURITY.md).

## Setting up

You need a recent stable Rust toolchain installed with `rustup`, plus a few cargo tools:

```sh
cargo install cargo-nextest cargo-deny --locked
```

The code is landing in stages. If a command below fails because the workspace is not there yet, that is why.

Run the same checks the project runs on every pull request:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
cargo deny check
```

If you touch the desktop app, also run `npm --prefix apps/desktop/ui test` and read [docs/development/desktop.md](docs/development/desktop.md).

## How we work

Tests come first. A change that alters behaviour starts with a failing test, followed by the smallest change that makes it pass. A bug fix starts with a test that reproduces the bug, committed before the fix.

Work happens on a short-lived branch named `<type>/<short-description>`, for example `fix/empty-file-version` or `docs/release-process`. Open a pull request against `main`.

## Commit messages

Every commit follows [Conventional Commits](https://www.conventionalcommits.org/): `<type>(<scope>): <description>`.

- Types: `feat`, `fix`, `perf`, `refactor`, `test`, `docs`, `build`, `ci`, `chore`, `revert`.
- The scope is the crate or area, for example `core`, `desktop`, `infra` or `ci`. It is optional.
- Add `!` before the colon for a breaking change.
- Keep the first line under 100 characters.

Pull requests are merged by rebasing, so each of your commits ends up on `main` exactly as you wrote it. Aim for one commit per step of your work, each of which builds and passes the tests. Do not leave `fixup!` or `squash!` commits behind, and do not merge `main` into your branch; rebase instead.

## Pull requests

A pull request can be merged when the `ci-success` check is green. Describe what changed and why, how you tested it, and whether the documentation needed an update.

## Licensing of contributions

By contributing you agree that your contribution is licensed under GPL-3.0-or-later.
