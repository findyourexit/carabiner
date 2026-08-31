# Support

## The 0.1.0 Pre-Release

The `0.1.0` release is a pre-release. The command-line interface, configuration file format, and programmatic API may change before a stable 1.0.0 release. Start with [Getting Started](docs/getting-started/installation.md) and report defects through the [issue tracker](https://github.com/findyourexit/carabiner/issues).

## Platform Support

Carabiner is built and tested on the following targets:

| Target | Status |
|---|---|
| AArch64 macOS (`aarch64-apple-darwin`) | Supported |
| x86_64 Linux (`x86_64-unknown-linux-gnu`) | Supported |
| x86_64 Windows (`x86_64-pc-windows-msvc`) | Supported |
| x86_64 macOS (`x86_64-apple-darwin`) | Build-only and best effort |
| AArch64 Linux (`aarch64-unknown-linux-gnu`) | Build-only and best effort |
| AArch64 Windows (`aarch64-pc-windows-msvc`) | Build-only and best effort |

## Usage Questions

Search the [documentation](docs/) and existing discussions first. Use [GitHub Discussions](https://github.com/findyourexit/carabiner/discussions) for installation, configuration, and usage questions.

## Troubleshooting

**`carabiner generate` produces no output:** Run `carabiner doctor` first. It performs read-only diagnostics on the project configuration file and reports misspelled keys, unknown targets, and other common configuration problems.

**Generated files differ from expected:** Use `carabiner generate --dry-run` to preview changes without writing files, or `carabiner generate --check` to verify that the current files are up to date without modifying anything.

**Permission or write errors:** Confirm that the current user has write access to the output directory. Generated files are written relative to the current working directory by default.

## Defects

Use the [issue chooser](https://github.com/findyourexit/carabiner/issues/new/choose) for a reproducible defect. Include the Carabiner version, operating system, the command you ran, the expected output, and the actual output or error message.

Do not use a public issue for a security vulnerability. See [SECURITY.md](SECURITY.md) for private reporting.
