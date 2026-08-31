# Plugin packaging

Carabiner can generate and import supported component files in Claude Code and Google Antigravity plugin directories. Use these project-scope targets when the generated files belong in a plugin instead of a consumer project or user configuration:

- `claudecode-plugin`
- `antigravity-plugin`

These targets are excluded from `--targets "*"`. Their output uses top-level component directories such as `skills/` and `rules/`, which could otherwise conflict with ordinary project directories.

## Generate plugin components

Pass the plugin directory with `--output-roots`:

```bash
carabiner generate \
  --targets claudecode-plugin \
  --features mcp,commands,subagents,skills,hooks \
  --output-roots ./plugins/review-tools

carabiner generate \
  --targets antigravity-plugin \
  --features rules,mcp,subagents,skills,hooks \
  --output-roots ./plugins/review-tools
```

You can keep target-specific output roots and features in `carabiner.jsonc`:

```jsonc
{
  "outputRoots": {
    "claudecode-plugin": "./plugins/claude-review-tools",
    "antigravity-plugin": "./plugins/antigravity-review-tools",
  },
  "targets": {
    "claudecode-plugin": ["mcp", "commands", "subagents", "skills", "hooks"],
    "antigravity-plugin": ["rules", "mcp", "subagents", "skills", "hooks"],
  },
}
```

Carabiner manages component files only. It does not create or modify plugin metadata, marketplace catalogs, scripts, or other package assets. Maintain the upstream metadata required for the plugin:

- Claude Code: `.claude-plugin/plugin.json` when the plugin uses a manifest.
- Antigravity: `plugin.json`.

Generation creates the directories required for supported components. Carabiner refuses to write through a symbolic-link output root or a symbolic-link path component. When `--delete` is enabled, it also refuses to delete through a symbolic-link managed directory.

With `--delete`, Carabiner can remove files in a selected directory-based component location when they are no longer generated. Keep hand-authored files outside component directories that Carabiner manages when using `--delete`.

## Import plugin components

Use `--output-root` to identify the plugin directory to read. Carabiner writes imported canonical files to `.carabiner/` in the current working directory:

```bash
carabiner import \
  --targets claudecode-plugin \
  --features mcp,commands,subagents,skills,hooks \
  --output-root ./plugins/review-tools

carabiner import \
  --targets antigravity-plugin \
  --features rules,mcp,subagents,skills,hooks \
  --output-root ./plugins/review-tools
```

The `convert` command does not support plugin packaging targets. Import from the source plugin first, then generate into the destination plugin with an explicit output root.

## Component paths

| Target | Rules | MCP | Commands | Subagents | Skills | Hooks |
| --- | --- | --- | --- | --- | --- | --- |
| `claudecode-plugin` | Not supported | `.mcp.json` | `commands/*.md` | `agents/*.md` | `skills/*/SKILL.md` | `hooks/hooks.json` |
| `antigravity-plugin` | `rules/*.md` | `mcp_config.json` | Not supported | `agents/*.md` | `skills/*/SKILL.md` | `hooks.json` |

For MCP and hook settings that apply only to a plugin target, use the exact target name in the source configuration. Use `claudecode-plugin` or `antigravity-plugin`, not `claudecode` or `antigravity-ide`.

## Claude Code plugin constraints

Claude Code applies some rules to plugin components that differ from project components.

- **Hook command paths:** When a hook command starts with `./`, Carabiner writes it relative to the plugin root. For example, `./scripts/fmt.sh` becomes `"$CLAUDE_PLUGIN_ROOT"/scripts/fmt.sh`. A command that already starts with a variable, such as `$CLAUDE_PROJECT_DIR/scripts/hook.sh`, is left unchanged. Use an explicit variable when the command should resolve from the consumer project instead of the plugin.
- **Subagent frontmatter:** For `claudecode-plugin`, Carabiner removes `hooks`, `mcpServers`, and `permissionMode` from generated subagent frontmatter. It retains `isolation` only when its value is `worktree`. Importing the plugin cannot restore fields that were not written, so keep the canonical `.carabiner/subagents/*.md` files as the source of truth.
- **Subagent names:** Claude Code scopes plugin agents as `<plugin>:<agent>`. Do not use `:` in an agent Markdown name so it is not ambiguous with that namespace.

See the [Claude Code plugins reference](https://code.claude.com/docs/en/plugins-reference) for upstream plugin requirements.

## Use a Claude Code plugin in JetBrains Junie

[Junie CLI Extensions](https://junie.jetbrains.com/docs/junie-cli-extensions.html) accepts both its native `.junie-extension/marketplace.json` format and the Claude-compatible `.claude-plugin/marketplace.json` format. A plugin generated with `claudecode-plugin` can be listed in a Claude-compatible marketplace and installed in Junie through `/extensions`.

Carabiner writes only the Claude Code component files for this target: `commands/`, `agents/`, `skills/`, `.mcp.json`, and `hooks/hooks.json`. You must author `.claude-plugin/plugin.json` and the marketplace catalog separately.
