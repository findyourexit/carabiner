<p align="center">
  <img src="assets/carabiner-header.png" alt="Carabiner, a unified CLI for managing AI rules and configuration across AI coding tools" />
</p>

# Carabiner

[![CI](https://github.com/findyourexit/carabiner/actions/workflows/ci.yml/badge.svg)](https://github.com/findyourexit/carabiner/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/carabiner.svg)](https://crates.io/crates/carabiner)
[![License](https://img.shields.io/badge/license-MIT-2f855a)](LICENSE)

A unified CLI for managing AI rules and configuration across AI coding tools.

Carabiner lets you author rules, commands, permissions, MCP server definitions, hooks, and subagent profiles once in a canonical source directory, then generate the tool-specific files that each assistant requires. When the tools your team uses change, you update one source and regenerate.

## Quick Start

### Install

<details>
<summary><strong>crates.io</strong></summary>

```console
cargo install carabiner --locked
carabiner --version
```

</details>

<details>
<summary><strong>Build from source</strong></summary>

```console
git clone https://github.com/findyourexit/carabiner.git
cd carabiner
cargo install --path . --locked
carabiner --version
```

</details>

### Set up a project

```console
# Scaffold the canonical source directory and a sample configuration file
carabiner init

# Generate tool-specific files for all configured targets
carabiner generate --targets "*" --features "*"
```

If you already have AI tool configuration files in your project, import them first:

```console
carabiner import --targets claudecode --features rules,mcp,commands,subagents
carabiner import --targets cursor --features rules
```

## What Carabiner Manages

Carabiner operates on a canonical source directory and a project configuration file at your project root. Everything in that source tree is under your control and can be committed to version control. Generated output files are written alongside your source code and remain readable by the AI tools even if Carabiner is not installed.

| Feature | Description |
|---|---|
| Rules | Guidance injected into the AI context |
| Commands | Slash command definitions |
| Subagents | Specialist agent profiles |
| Skills | Reusable skill bundles |
| MCP servers | Model context protocol server list |
| Hooks | Pre- and post-tool-use shell hooks |
| Permissions | Tool allow and deny rules |
| Ignore | Paths excluded from AI context |

## Supported Tools

The tables below show which features each tool supports. A checkmark means the feature is supported in at least one scope (project or global). See the [Supported Tools reference](docs/reference/supported-tools.md) for the `--targets` value for each tool and full scope breakdowns.

### AI Coding Tools

| Tool | rules | ignore | mcp | commands | subagents | skills | hooks | permissions | checks |
|---|:---:|:---:|:---:|:---:|:---:|:---:|:---:|:---:|:---:|
| Amp | ✅ | | ✅ | | | ✅ | ✅ | ✅ | ✅ |
| AugmentCode | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Claude Code | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | |
| Cline | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | |
| Codex CLI | ✅ | | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | |
| Cursor | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| DeepAgents | ✅ | | ✅ | | ✅ | ✅ | ✅ | | |
| Devin Desktop | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | |
| Factory Droid | ✅ | | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | |
| GitHub Copilot | ✅ | | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | |
| GitHub Copilot CLI | ✅ | | ✅ | | ✅ | ✅ | ✅ | ✅ | |
| Google Antigravity CLI | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | |
| Google Antigravity IDE | ✅ | | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | |
| Goose | ✅ | | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | |
| Grok CLI | ✅ | | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | |
| Hermes Agent | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| JetBrains AI Assistant | ✅ | ✅ | ✅ | | | ✅ | | | |
| JetBrains Junie | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | |
| Kilo Code | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | |
| Kimi Code | ✅ | | ✅ | | ✅ | ✅ | ✅ | ✅ | |
| Kiro CLI | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | |
| Kiro IDE | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | |
| Meta Muse Code | ✅ | | | | | ✅ | | | |
| OpenCode | ✅ | | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | |
| Pi Coding Agent | ✅ | | | ✅ | | ✅ | ✅ | ✅ | |
| Qwen Code | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | |
| Reasonix | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | |
| Replit | ✅ | | | | | ✅ | | | |
| Roo Code ⚠️ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | | | |
| Rovo Dev | ✅ | | ✅ | ✅ | ✅ | ✅ | | ✅ | ✅ |
| Takt | ✅ | | ✅ | ✅ | ✅ | ✅ | | ✅ | ✅ |
| Vibe Code | ✅ | ✅ | ✅ | | ✅ | ✅ | ✅ | ✅ | |
| Warp | ✅ | ✅ | ✅ | ✅ | | ✅ | | ✅ | |
| Zed | ✅ | ✅ | ✅ | | | ✅ | | ✅ | |
| Zoo Code | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | | ✅ | |

### Legacy and Plugin Targets

These targets are excluded from `--targets "*"`. Use them only by naming them explicitly.

| Tool | rules | ignore | mcp | commands | subagents | skills | hooks | permissions | checks |
|---|:---:|:---:|:---:|:---:|:---:|:---:|:---:|:---:|:---:|
| AugmentCode (legacy) ⚠️ | ✅ | | | | | | | | |
| Claude Code (legacy) ⚠️ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | | | |
| Claude Code plugin | | | ✅ | ✅ | ✅ | ✅ | ✅ | | |
| Google Antigravity plugin | ✅ | | ✅ | | ✅ | ✅ | ✅ | | |
| Kiro ⚠️ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | |

### Open Standards

| Tool | rules | ignore | mcp | commands | subagents | skills | hooks | permissions | checks |
|---|:---:|:---:|:---:|:---:|:---:|:---:|:---:|:---:|:---:|
| AGENTS.md | ✅ | | | (s) | (s) | (s) | | | |
| AgentsSkills | | | | | | ✅ | | | |

**Notes:**
- ⚠️ Deprecated. Roo Code is end-of-life; use `zoocode`. Kiro is a deprecated alias; use `kiro-cli` or `kiro-ide`. AugmentCode and Claude Code legacy targets use older configuration formats; use `augmentcode` and `claudecode`.
- (s) Simulated — the feature is generated as instructions inside the root rule, not as native tool files.
- The `ignore` feature is deprecated. Use `permissions` with `read` deny rules instead.
- Some features are only available in one scope. A ✅ means supported in at least one of project scope or global scope.

## Commands

| Command | Description |
|---|---|
| `carabiner init` | Scaffold the source directory with sample files |
| `carabiner generate` | Write tool-specific files from your canonical sources |
| `carabiner import` | Convert a tool's native config into canonical form |
| `carabiner convert` | Convert directly between two tool formats |
| `carabiner add` | Add a remote source to your configuration |
| `carabiner fetch` | Fetch rules or skills from a remote source |
| `carabiner install` | Install all sources declared in the project configuration file |
| `carabiner gitignore` | Append generated-file paths to `.gitignore` |
| `carabiner doctor` | Diagnose configuration problems |
| `carabiner docs` | Read documentation in the terminal |
| `carabiner update` | Update Carabiner to the latest release |
| `carabiner mcp` | Run Carabiner as an MCP server |

Run `carabiner <command> --help` for full option details.

## Documentation

- [Getting Started](docs/getting-started/installation.md)
- [Quick Start](docs/getting-started/quick-start.md)
- [Configuration](docs/guide/configuration.md)
- [Declarative Sources](docs/guide/declarative-sources.md)
- [Separate Input Roots](docs/guide/separate-input-root.md)
- [Global Mode](docs/guide/global-mode.md)
- [Plugin Packaging](docs/guide/plugin-packaging.md)
- [Dry Run and Check Mode](docs/guide/dry-run.md)
- [CLI Commands Reference](docs/reference/cli-commands.md)
- [Supported Tools Reference](docs/reference/supported-tools.md)
- [File Formats Reference](docs/reference/file-formats.md)
- [MCP Server](docs/reference/mcp-server.md)
- [Programmatic API](docs/api/programmatic-api.md)
- [FAQ](docs/faq.md)

## Development

```console
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```
