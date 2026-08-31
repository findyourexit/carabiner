# Security Policy

## Supported Versions

Carabiner supports the latest stable release. Security fixes are applied to the current release only.

| Version | Status |
|---|---|
| `0.1.x` | Current pre-release line |
| `main` | Development only |

## Report Privately

Use GitHub's **Security** tab and **Report a vulnerability** as the primary private reporting channel. Do not open a public issue for an unpatched vulnerability.

Report issues such as:

- Path traversal in source file discovery or output writing
- Arbitrary file write or read outside intended directories
- Command injection through user-controlled input
- Information disclosure in generated output
- Dependency or release-integrity vulnerabilities
- Any defect that causes files to be written to unintended locations

If private vulnerability reporting is unavailable, email the lead maintainer at `tom.larcher@gmail.com`, the public maintainer contact listed in `Cargo.toml`.

## What to Include

Provide the following when it is safe to do so:

- The affected version or commit and operating system
- Exact reproduction steps and commands
- Expected and actual behavior
- Whether any unintended files were written or read
- Any requested disclosure constraints

## Response Policy

Reports are handled on a best-effort basis. Valid reports receive coordinated fixes and advisories when appropriate. No fixed response time is promised.

## Scope

Hostile filenames, symlinks, and unexpected file system layouts used as input to Carabiner commands are in scope. Carabiner trusts the operating-system kernel and the contents of source files you have authored.
