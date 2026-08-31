# Contributing to Carabiner

Thank you for improving Carabiner. Contributions are welcome across Rust code, tests, documentation, and tool-target support.

## Before opening work

- Search existing issues and pull requests first.
- Use the issue chooser for reproducible defects and feature proposals.
- For a substantial change, open an issue before implementation so the scope and public behavior can be agreed without wasting work.

## Development setup

Carabiner uses Rust 1.88 and the 2021 edition. The pinned toolchain is declared in `rust-toolchain.toml`.

```console
cargo check --all-targets --locked
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
cargo fmt --all -- --check
```

## Engineering expectations

- New tool targets must implement both generate and import paths and be covered by at least one integration test.
- Engine changes that affect idempotency must include a test that generates twice and asserts zero output files on the second run.
- Runtime behavior must remain local: no telemetry or outbound network access outside user-invoked fetch and install commands.
- Add tests for new observable behavior and plausible regressions. Avoid tests coupled only to implementation details.
- Update documentation when user-visible behavior changes.

## Pull requests

Keep pull requests focused and reviewable. Include:

1. the user-visible problem or feature;
2. the chosen behavior and important tradeoffs;
3. exact verification commands and results; and
4. documentation updates if the change affects a user-facing contract.

## Commit attribution

Carabiner does not require a contributor license agreement or copyright assignment. To enable the repository's Conventional Commit template in this clone:

```console
git config --local commit.template .gitmessage
```

## Conduct

Participation is governed by [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
