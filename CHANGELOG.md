# Changelog

All notable Carabiner changes are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

## [Unreleased]

### Added

- Starter `project-context` skill collection under `skills/`, fetchable with `carabiner fetch findyourexit/carabiner`.

### Changed

- Documentation, schemas, and contributor workflows now reflect the current 0.1.x behavior and validation contracts.
- Unknown top-level canonical hook events are rejected instead of being passed through silently.
- Canonical MCP sources now reject a server that defines both `type` and `transport`, removing adapter-dependent transport precedence.
- Canonical MCP sources now reject incomplete explicit transports and ambiguous command/URL inference; malformed shared permission maps now fail instead of silently generating no permissions.
- GitHub tokens now authenticate HTTPS GitHub source clones for `fetch`, `add`, and every `install` mode without exposing the token in the clone URL.
- Claude Code MCP output now normalizes canonical transport and connection aliases, preventing remote servers from being emitted without the explicit `type` required by Claude Code.
- Adopted the Contributor Covenant Code of Conduct, version 2.1.

## [0.1.5] - 2026-09-08

### Fixed

- Windows now uses `USERPROFILE` when `HOME` is unset, restoring Hermes Agent project generation and Windows compatibility coverage.

### Notes

- This release reissues the 0.1.4 Windows compatibility fix after release metadata recovery.

## [0.1.4] - 2026-09-08

### Fixed

- Windows now uses `USERPROFILE` when `HOME` is unset, restoring Hermes Agent project generation and Windows compatibility coverage.

## [0.1.3] - 2026-09-08

### Fixed

- Release builds now install Cross through Bash on Windows cross-compilation targets.

## [0.1.2] - 2026-09-08

### Fixed

- MCP file operations now reject symlinked destination paths, including nested skill files and deletes.
- `install --frozen` now validates cached artifact hashes and never fetches mutable source state.
- The release workflow now publishes the raw platform assets consumed by `carabiner update`.
- Codex CLI project capabilities no longer advertise unsupported project-scoped commands.

### Changed

- npm installs require HTTPS, enforce integrity metadata, use in-process hashing and hardened archive extraction, and require local opt-in before sending credentials to private registries.
- CI now verifies the declared Rust MSRV; release and documentation workflows use pinned tools, dependencies, and least-privilege jobs.
- YAML parsing now uses the maintained `serde_yaml_ng` fork.

## [0.1.1] - 2026-08-31

### Added

- Homebrew installation via `brew tap findyourexit/tap && brew install carabiner`. Prebuilt binaries for macOS (Apple Silicon and Intel) are available on the tap.
- Compatibility matrix in the README showing supported features per tool across all 42 targets.
- Documentation website at [tomlarcher.com/carabiner](https://tomlarcher.com/carabiner/), built with Zensical and published automatically on push to `main`.

## [0.1.0] - 2026-08-31

- Unified AI rules management CLI with support for generating and importing configuration for 42 AI coding tool targets.
- Rule, command, subagent, skill, MCP server, hooks, permissions, and ignore generation for each configured target.
- `generate`, `import`, `convert`, `add`, `fetch`, `install`, `init`, `gitignore`, `doctor`, `docs`, `update`, `release-notes`, and `mcp` subcommands.
- Canonical source directory and project configuration file for authoring all features in one place.
- Declarative source management with a lockfile for reproducible remote rule and skill installs.
- `--dry-run` and `--check` modes for non-destructive previews and CI verification.
- Global mode for writing to user-scope tool configuration paths.
- `--input-roots` for decoupling the canonical source location from the output root.
- JSON output mode (`--json`) for all commands.
- MCP server mode (`carabiner mcp`) for driving Carabiner from a model context protocol client.
- Built-in documentation viewer (`carabiner docs`).
- Programmatic Rust API via the `carabiner` library crate.
