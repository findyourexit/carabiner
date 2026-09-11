<p align="center">
  <img src="https://raw.githubusercontent.com/findyourexit/carabiner/main/assets/carabiner-header.png" alt="Carabiner, a unified CLI for managing AI rules and configuration across AI coding tools" />
</p>

# Carabiner

Carabiner is a command-line tool for managing AI rules and configuration across AI coding tools. Author your rules, commands, permissions, MCP server definitions, hooks, and subagent profiles once in a canonical source directory, then generate the tool-specific files each assistant requires.

## Get Started

Install Carabiner and scaffold a new project:

```console
cargo install carabiner --locked
carabiner init
carabiner generate
```

See [Installation](getting-started/installation.md) for Homebrew and build-from-source options, and [Quick Start](getting-started/quick-start.md) for a guided walkthrough.

## What It Manages

Carabiner reads from a `.carabiner/` source directory and a `carabiner.jsonc` configuration file in your project root. Generated output files are plain configuration that your AI tools read directly — they continue to work even if Carabiner is not installed.

| Feature | Description |
|---|---|
| Rules | Guidance injected into the AI context |
| Commands | Slash command definitions |
| Subagents | Specialist agent profiles |
| Skills | Reusable skill bundles |
| MCP servers | Model context protocol server list |
| Hooks | Pre- and post-tool-use shell hooks |
| Permissions | Tool allow and deny rules |
| Checks | Code-review and quality-gate instructions |
| Ignore | Legacy path exclusions; prefer permissions for new projects |

Carabiner generates and imports configuration for 42 supported target names, including AI coding tools and open standards. See the [Supported Tools reference](reference/supported-tools.md) for the full compatibility matrix.

## Source

- [GitHub repository](https://github.com/findyourexit/carabiner)
- [crates.io package](https://crates.io/crates/carabiner)
