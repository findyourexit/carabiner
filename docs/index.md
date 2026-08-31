<p align="center">
  <img src="https://raw.githubusercontent.com/findyourexit/carabiner/main/assets/carabiner-header.png" alt="Carabiner — unified AI rules management CLI" />
</p>

# Carabiner

Carabiner is a command-line tool for managing AI rules and configuration across AI coding tools. Author your rules, commands, permissions, MCP server definitions, hooks, and subagent profiles once in a canonical source directory, then generate the tool-specific files each assistant requires.

## Get Started

Install Carabiner and scaffold a new project:

```console
cargo install carabiner --locked
carabiner init
carabiner generate --targets "*" --features "*"
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

## Supported Tools

Carabiner generates and imports configuration for 42 AI coding tools. See the [Supported Tools reference](reference/supported-tools.md) for the full compatibility matrix.

## Source

- [GitHub repository](https://github.com/findyourexit/carabiner)
- [crates.io package](https://crates.io/crates/carabiner)
