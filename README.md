# Folyvo

Folyvo keeps every saved version of the files you work on, lets you mark the one that is ready to share, and records where each shared copy went. It runs on your own computer, needs no account and sends nothing anywhere.

The idea is to give people who work with documents the guarantees that developers get from Git, without the vocabulary.

[![CI](https://github.com/bilalyazicioglu/Folyvo/actions/workflows/ci.yml/badge.svg)](https://github.com/bilalyazicioglu/Folyvo/actions/workflows/ci.yml)
[![OpenSSF Scorecard](https://api.scorecard.dev/projects/github.com/bilalyazicioglu/Folyvo/badge)](https://scorecard.dev/viewer/?uri=github.com/bilalyazicioglu/Folyvo)
[![License: GPL-3.0-or-later](https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg)](LICENSE)

## Status

Folyvo is under development and nothing has been released yet. The first target is macOS on Apple Silicon.

## Verifying a release

Every release asset carries a build provenance attestation and ships with an SBOM and a `SHA256SUMS` file. To check a downloaded file:

```sh
gh attestation verify <file> --repo bilalyazicioglu/Folyvo
shasum -a 256 -c SHA256SUMS
```

## Documentation

The documentation lives in [`docs/`](docs/) and is mirrored to the project wiki.

## Contributing

Contributions are welcome. Please read [CONTRIBUTING.md](CONTRIBUTING.md) first, and note the [code of conduct](CODE_OF_CONDUCT.md). Security problems should be reported privately, as described in [SECURITY.md](SECURITY.md).

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
