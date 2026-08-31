# Supported Tool Targets

Carabiner supports generation and import for the 42 target names below. Use target names exactly as shown with `--targets`.

## Install and select targets

Install Carabiner with:

```sh
cargo install carabiner --locked
```

For example, generate project-scoped output for Claude Code and Cursor with:

```sh
carabiner generate --targets claudecode,cursor
```

Use `carabiner import --targets <target>` to import a supported target configuration. The matrix records generation support. `Project` means project mode, `Global` means `--global` mode, and `Project and global` means both scopes. An empty cell means that feature is not supported for that target and scope.

<!-- SUPPORTED_TOOLS_DOCS:BEGIN -->

| Tool | Target | Rules | Ignore | MCP | Commands | Subagents | Skills | Hooks | Permissions | Checks |
| --- | --- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| AGENTS.md | `agentsmd` | Project |  |  | Project, simulated | Project, simulated | Project, simulated |  |  |  |
| AgentsSkills | `agentsskills` |  |  |  |  |  | Project and global |  |  |  |
| Amp | `amp` | Project and global |  | Project and global |  |  | Project and global | Project and global | Project and global | Project and global |
| Claude Code | `claudecode` | Project and global | Project | Project and global | Project and global | Project and global | Project and global | Project and global | Project and global |  |
| Claude Code legacy | `claudecode-legacy` | Project and global | Project | Project and global | Project and global | Project and global | Project and global |  |  |  |
| Claude Code plugin | `claudecode-plugin` |  |  | Project | Project | Project | Project | Project |  |  |
| Codex CLI | `codexcli` | Project and global |  | Project and global, preserves tool selection | Global | Project and global | Project and global | Project and global | Project and global |  |
| GitHub Copilot | `copilot` | Project and global |  | Project | Project | Project and global | Project and global | Project and global | Project |  |
| GitHub Copilot CLI | `copilotcli` | Project and global |  | Project and global, preserves tool selection |  | Project and global | Project and global | Project and global | Project and global |  |
| Goose | `goose` | Project and global |  | Project and global | Project and global | Project and global | Project | Project and global | Global |  |
| Hermes Agent | `hermesagent` | Project | Project | Global, preserves tool selection | Global | Project and global | Global | Global | Global | Project |
| Grok CLI | `grokcli` | Project and global |  | Project and global | Project and global | Project and global | Project and global | Project and global | Project and global |  |
| Cursor | `cursor` | Project | Project | Project and global | Project and global | Project and global | Project and global | Project and global | Project and global | Project |
| deepagents-cli | `deepagents` | Project and global |  | Project and global, preserves tool selection |  | Project and global | Project and global | Project and global |  |  |
| Factory Droid | `factorydroid` | Project and global |  | Project and global, preserves tool selection | Project and global | Project and global | Project and global | Project and global | Project and global |  |
| OpenCode | `opencode` | Project and global |  | Project and global, preserves tool selection | Project and global | Project and global | Project and global | Project and global | Project and global |  |
| Cline | `cline` | Project and global | Project | Global | Project and global | Project and global | Project and global | Project and global | Project |  |
| Kilo Code | `kilo` | Project and global | Project | Project and global, preserves tool selection | Project and global | Project and global | Project and global | Project and global | Project and global |  |
| Kimi Code | `kimi-code` | Project and global |  | Project and global, preserves tool selection |  | Project and global | Project and global | Global | Global |  |
| Roo Code legacy | `roo` | Project and global | Project | Project, preserves tool selection | Project and global | Project | Project and global |  |  |  |
| Zoo Code | `zoocode` | Project and global | Project | Project, preserves tool selection | Project and global | Project | Project and global |  | Project |  |
| Rovodev (Atlassian) | `rovodev` | Project and global |  | Project and global | Project and global | Project and global | Project and global |  | Project and global | Project |
| Takt | `takt` | Project and global |  | Project and global | Project and global | Project and global | Project and global |  | Project and global | Project and global |
| Vibe Code | `vibe` | Project and global | Project | Project and global, preserves tool selection |  | Project and global | Project and global | Project and global | Project and global |  |
| Qwen Code | `qwencode` | Project and global | Project | Project and global, preserves tool selection | Project and global | Project and global | Project and global | Project and global | Project and global |  |
| Meta Muse Code | `musecode` | Project |  | Global |  |  | Project and global |  |  |  |
| Reasonix | `reasonix` | Project and global | Project and global | Project and global | Project and global | Project and global | Project and global | Project and global | Project and global |  |
| Kiro legacy | `kiro` | Project and global | Project and global | Project and global, preserves tool selection | Project | Project | Project | Project | Project |  |
| Kiro CLI | `kiro-cli` | Project and global | Project and global | Project and global, preserves tool selection | Project and global | Project and global | Project and global | Project and global | Project |  |
| Kiro IDE | `kiro-ide` | Project and global | Project and global | Project and global, preserves tool selection | Project | Project and global | Project and global | Project and global | Project |  |
| Google Antigravity IDE | `antigravity-ide` | Project and global |  | Project and global, preserves tool selection | Project and global | Project and global | Project and global | Project and global | Project |  |
| Google Antigravity CLI | `antigravity-cli` | Project and global | Project | Project and global, preserves tool selection | Project and global | Project and global | Project and global | Project and global | Global |  |
| Google Antigravity plugin | `antigravity-plugin` | Project |  | Project, preserves tool selection |  | Project | Project | Project |  |  |
| JetBrains AI Assistant | `aiassistant` | Project | Project | Project and global |  |  | Project |  |  |  |
| JetBrains Junie | `junie` | Project and global | Project | Project and global | Project and global | Project and global | Project and global | Global | Global |  |
| AugmentCode | `augmentcode` | Project and global | Project | Project and global | Project and global | Project and global | Project and global | Project and global | Project and global | Project |
| AugmentCode legacy | `augmentcode-legacy` | Project |  |  |  |  |  |  |  |  |
| Devin Desktop | `devin` | Project and global | Project and global | Project and global, preserves tool selection | Project and global | Project and global | Project and global | Project and global | Project and global |  |
| Warp | `warp` | Project and global | Project | Project and global | Project and global |  | Project and global |  | Global |  |
| Replit | `replit` | Project |  |  |  |  | Project and global |  |  |  |
| Pi Coding Agent | `pi` | Project and global |  |  | Project and global |  | Project and global | Project and global | Project and global |  |
| Zed | `zed` | Project and global | Project and global | Project and global |  |  | Project and global |  | Project and global |  |

<!-- SUPPORTED_TOOLS_DOCS:END -->

`Project, simulated` requires the corresponding `--simulate-commands`, `--simulate-subagents`, or `--simulate-skills` option in project mode.

`Preserves tool selection` identifies MCP targets that preserve supported per-server tool-selection settings. Depending on the target, this includes `enabledTools`, `disabledTools`, or both.

`Legacy` identifies a legacy or deprecated target. See the target notes below.

## Hermes Agent

The `hermesagent` target is validated against Hermes Agent v0.20.2 (release `v2026.8.16`). The supported contract includes project rules, ignore patterns, subagents, and checks. It also includes global MCP servers, commands, subagents, skills, hooks, and permissions. Generation, `--check`, and import round-trips are covered for both advertised scopes.

Carabiner honors Hermes profiles through `HERMES_HOME`. When it is set, its value is the profile root. Global configuration is read and written directly under `$HERMES_HOME`, including `config.yaml`, `skills/`, `plugins/`, and `carabiner/`. Carabiner does not append `.hermes`. When `HERMES_HOME` is unset, Carabiner follows Hermes's platform default: `~/.hermes` except on Windows, where it uses `%LOCALAPPDATA%\hermes`. Because `HERMES_HOME` identifies the profile Hermes reads, it takes precedence over `--output-roots` in global scope. Project-scoped paths remain rooted in the project.

Changing the resolved profile root leaves generated files under the prior root. `--delete` reconciles only the root resolved by the current run. Files under a root that no longer resolves are not visible to Carabiner and must be removed manually after confirming that Hermes no longer reads them. This applies when `HERMES_HOME` is set or changed. It also applies to two upgrades that changed the resolved root: before v16.0.0 global files went to `~/.hermes` even when `HERMES_HOME` was set, and before v16.2.0 they went there on Windows rather than to `%LOCALAPPDATA%\hermes`.

Project plugins are registered by adding their names to `$HERMES_HOME/config.yaml`. Carabiner does not persist Hermes's global project-plugin trust gate. Run Hermes from a trusted project root with `HERMES_ENABLE_PROJECT_PLUGINS=true` for an explicit session-scoped opt-in. A future Hermes release that changes its loaders, schemas, or plugin API requires new compatibility validation.

## Target notes

### Target selection

`--targets '*'` excludes `augmentcode-legacy`, `claudecode-legacy`, `antigravity-plugin`, and `claudecode-plugin`. Select one of these targets explicitly when it is needed.

`augmentcode-legacy` supports project rules only. It writes the root rule to `.augment-guidelines` and non-root rules to `.augment/rules/`. Do not select it with `augmentcode`.

`claudecode-legacy` uses `.claude/memories` for non-root project rules. It cannot be selected with `claudecode`.

### Google Antigravity

Antigravity 2.0 separates the desktop `antigravity-ide` product from the `antigravity-cli` (`agy`) product.

- The IDE reads global MCP configuration and skills from the shared `~/.gemini/config/` tree: `~/.gemini/config/mcp_config.json` and `~/.gemini/config/skills/`. The CLI also uses the shared global MCP file, while its global skills directory is `~/.gemini/antigravity-cli/skills/`.
- Both targets share the global rule file `~/.gemini/GEMINI.md` and global hooks file `~/.gemini/config/hooks.json`. Generating both targets in global mode writes each shared file once.
- Both targets write the project root rule as `AGENTS.md` and non-root rules under `.agents/rules/`. Gemini-lineage tools discover rules in the order `AGENTS.md`, `CONTEXT.md`, then `GEMINI.md`. The IDE has read `AGENTS.md` since v1.20.3. The IDE adds trigger frontmatter to non-root rules, while the CLI writes plain Markdown.
- Both targets use `.agents/workflows/` for project workflows, invoked as `/workflow-name`. In global mode, the IDE uses `~/.gemini/antigravity/global_workflows/` and the CLI uses `~/.gemini/antigravity-cli/global_workflows/`.

### Kiro

Kiro IDE reads Markdown subagents from `.kiro/agents/*.md` and structured JSON hooks from `.kiro/hooks/*.json`, with the form `{ "version": "v1", "hooks": [ ... ] }`. Kiro CLI reads JSON agent-config subagents from `.kiro/agents/*.json`. The `kiro-cli` and `kiro-ide` targets exist because one target cannot emit both subagent formats faithfully. The `kiro` target is a deprecated alias whose mixed output remains unchanged for backward compatibility.

The `kiro-cli` and `kiro-ide` targets share steering rules with `inclusion`, `.kiro/settings/mcp.json` MCP configuration, `.kiro/prompts/` commands, `.kiro/skills/`, `.kiroignore`, and permissions. They differ only in the subagent format.

Both targets write all generated hooks to one `.kiro/hooks/carabiner.json` file in project scope and global scope. Its `hooks` array maps canonical lifecycle events to Kiro's PascalCase triggers: `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, and `Stop`. It supports both `agent` prompt actions and `command` actions. Kiro CLI 3.0 [migrated to this format](https://kiro.dev/docs/cli/v3/hooks-migration/) and no longer reads embedded agent hooks in `.kiro/agents/default.json`. Only the deprecated `kiro` alias still writes those hooks, including the `cacheTtl` to `cache_ttl_seconds` mapping.

Global skills at `~/.kiro/skills/`, global ignore at `~/.kiro/settings/kiroignore`, and global Kiro IDE subagents at `~/.kiro/agents/` are supported. Global Kiro CLI commands and subagents use `~/.kiro/prompts/` and `~/.kiro/agents/`. Kiro's shared MCP file preserves per-server `disabledTools`.

### Roo Code

Roo Code is end of life. Its final release was v3.54.0 on 2026-05-15, and the [Roo-Code repository](https://github.com/RooCodeInc/Roo-Code) is archived. New projects should use `zoocode`. [Zoo Code](https://github.com/Zoo-Code-Org/Zoo-Code) is the community continuation named by the Roo shutdown notice and continues Roo's release numbering.

The `roo` target remains supported because Zoo Code still reads the `.roo/` project tree and `~/.roo` global tree. Existing `roo` output continues to work, but it does not include features Zoo Code added after the fork. The two targets write the same files, so enable only one in a project. See the Zoo Code note in [File formats](./file-formats.md) for the fail-open hazard when `carabiner generate --targets roo` creates a shared `.roomodes` file.
