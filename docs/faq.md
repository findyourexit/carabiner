# FAQ

## Why does `carabiner generate` not produce the expected output?

Run `carabiner doctor` first. It performs read-only checks on `carabiner.jsonc` and `carabiner.local.jsonc`. It reports problems that generation tolerates, especially misspelled or unknown configuration keys. The configuration schema is non-strict, so a typo such as `"target"` instead of `"targets"` can otherwise be ignored and generation can fall back to defaults. See the [Doctor Command](./reference/cli-commands.md#doctor-command) reference for all checks.

## Why does the generated `.mcp.json` not work in Claude Code?

Add the following setting to `.claude/settings.json` or `.claude/settings.local.json` if you want Claude Code to automatically approve every project MCP server:

```diff title=".claude/settings.json"
{
+ "enableAllProjectMcpServers": true
}
```

According to [Claude Code's settings documentation](https://code.claude.com/docs/en/settings), this setting automatically approves all MCP servers defined in project `.mcp.json` files.

## Why does Google Antigravity not load rules when `.agents` directories are in `.gitignore`?

Google Antigravity does not load rules, workflows, or skills when `.agents/rules/`, `.agents/workflows/`, or `.agents/skills/` is listed in `.gitignore`, even when Agent Gitignore Access is enabled.

Antigravity 2.0 uses the plural `.agents/` directory by default. This applies to the `antigravity-ide` and `antigravity-cli` targets.

Add these paths to `.git/info/exclude` instead of `.gitignore`:

```bash
# Remove from .gitignore if present.
# **/.agents/rules/
# **/.agents/workflows/
# **/.agents/skills/

# Add to .git/info/exclude.
echo "**/.agents/rules/" >> .git/info/exclude
echo "**/.agents/workflows/" >> .git/info/exclude
echo "**/.agents/skills/" >> .git/info/exclude
```

`.git/info/exclude` behaves like `.gitignore` but is local to one clone. It keeps these directories out of Git without preventing Antigravity from loading them. Because it is not committed, each team member must add the entries locally.

## Why does Codex CLI deny SSH-agent access, temporary-directory writes, or reads of its own configuration?

By default, Carabiner generates the `[permissions.carabiner]` profile in `.codex/config.toml`, which extends Codex CLI's `:workspace` baseline. That baseline is intentionally restrictive, so a workflow can still be denied access to an SSH-agent socket, a temporary directory, or `~/.codex`.

Carabiner adds `".git/**" = "write"` under `:workspace_roots` by default. Set `codexcli.git_write_rules` to `false` to disable that carve-out. The rule makes the entire `.git` subtree, including `.git/config`, writable. This supports commands such as `git remote add`, `git push -u`, and repository-local `git config`. For stricter isolation, add a more specific canonical permission such as `read: { ".git/config": "allow" }`.

Only add the access that your workflow requires. Network and filesystem settings have different ownership rules.

### Network settings

Edit non-domain network settings directly in `.codex/config.toml`. Carabiner manages webfetch-derived domain rules. When it does not generate an allow-domain rule, it preserves a user-authored `network.enabled` value. It also preserves unrecognized network keys, including `dangerously_allow_all_unix_sockets`, and warns when it does so.

```toml title=".codex/config.toml"
[permissions.carabiner.network]
enabled = true
# Broad option that allows all Unix sockets.
dangerously_allow_all_unix_sockets = true

# Narrower option that allows only the SSH-agent socket.
# Replace this path with the value of $SSH_AUTH_SOCK on your machine.
# Codex does not expand environment variables in these keys.
# [permissions.carabiner.network.unix_sockets]
# "/path/to/ssh-agent.sock" = "allow"
```

### Filesystem settings

Author filesystem entries in `.carabiner/permissions.jsonc`, not in `.codex/config.toml`. Carabiner fully manages the profile's `filesystem` table, so hand-written entries there are replaced by the next `carabiner generate`. Put rules in the canonical configuration and use the tool-scoped `codexcli.permission` block so they apply only to Codex CLI.

```jsonc title=".carabiner/permissions.jsonc"
{
  "permission": {
    // ... shared rules ...
  },
  "codexcli": {
    "permission": {
      "write": {
        ".": "allow",
        ".git/**": "allow",
        ".agents/**": "allow",
        ".codex/**": "allow",
        ":root": "allow",
        ":minimal": "allow",
        ":tmpdir": "allow",
        ":slash_tmp": "allow",
      },
      "read": { "~/.codex/**": "allow", "~/.codex/auth.json": "deny" },
    },
  },
}
```

This example is deliberately broad. The `":root"` and `":minimal"` write pair grants the sandbox full disk write access.

These rules generate `"." = "write"`, `".git/**" = "write"`, `".agents/**" = "write"`, and `".codex/**" = "write"` under `:workspace_roots`. They also generate `":root" = "write"`, `":minimal" = "write"`, `":tmpdir" = "write"`, `":slash_tmp" = "write"`, `"~/.codex/**" = "read"`, and `"~/.codex/auth.json" = "deny"`. The configuration round-trips through `carabiner import` with two import exceptions. Keep the following configuration rule in mind as well:

- The default `".git/**" = "write"` carve-out is skipped during import because Carabiner adds it on every generation. If you disable `codexcli.git_write_rules` after importing, add `".git/**": "allow"` to the canonical configuration yourself.
- `":minimal"` is never imported, regardless of its value. Carabiner normally emits it as `"read"`, so re-add `":minimal": "allow"` to the canonical configuration after import if it must remain writable.
- A `codexcli.permission` block replaces the shared `permission` block for Codex CLI. Repeat any shared `read` or `write` rules there when they must also apply to Codex CLI.

What the entries do:

`Unix socket access`
:   `git push` and `git fetch` over SSH need the SSH-agent socket. `dangerously_allow_all_unix_sockets = true` is environment-independent but broad. A `unix_sockets` allow entry for the resolved `$SSH_AUTH_SOCK` path is narrower.

`.` / `.git/**` / `.agents/**` / `.codex/**` write
:   This is a practical workspace write set. `"."` makes workspace-subtree write access explicit because a tool-scoped category replaces the shared block. `".git/**"` is the default carve-out. `.agents/**` and `.codex/**` add access because the `:workspace` baseline keeps those directories read-only. Without them, a Codex session cannot update agent files or its project-level configuration. Write access to `.codex/**` lets a compromised session alter the configuration used by a later run. Write access to `.agents/**` lets it persist unwanted rules or skills. Omit either entry if in-session writes are unnecessary.

`:root` / `:minimal` write
:   Package runners and development tools can write user-level caches such as `~/.cache` and `~/.local`, which the `:workspace` baseline denies. `":root" = "write"` alone is not sufficient because Carabiner normally emits `":minimal" = "read"`. Codex treats that entry as narrowing the broader `:root` grant. Set both to `write` when full disk write access is required. This also permits writes to platform system paths, shell startup files, and programs on `PATH`. Prefer narrower home-directory patterns such as `"~/.cache/**"` when they are sufficient.

`:tmpdir` / `:slash_tmp` write
:   These allow writes to `$TMPDIR` and `/tmp`, which some build tools require.

`~/.codex/**` read with `auth.json` deny
:   Codex can read its configuration tree without reading credentials. Codex expands tilde paths, so no `$HOME` substitution is needed.

`glob_scan_max_depth`
:   Carabiner automatically emits Codex's default value of `8` when generated workspace-root rules contain an unbounded `**` pattern. The default `.git/**` carve-out is one such pattern.

See the [Codex permissions reference](https://developers.openai.com/codex/permissions) for the complete path and network syntax.

## How can I reduce generated-rule noise in pull request diffs?

AI coding tools need to read generated rule files from the working tree, so Carabiner does not add them to `.gitignore`. With many configured targets, those files can dominate a pull request diff.

Add generated paths to `.gitattributes` with GitHub's [`linguist-generated`](https://docs.github.com/en/repositories/working-with-files/managing-files/customizing-how-changed-files-appear-on-github#marking-files-as-generated) attribute. GitHub then collapses the files by default in pull requests while the files remain tracked and readable by the tools.

For example, a repository using `.agent/`, Claude Code, Cursor, and Copilot targets can use:

``` title=".gitattributes"
.agent/rules/**           linguist-generated
.agent/skills/**          linguist-generated
.agent/workflows/**       linguist-generated
CLAUDE.md                 linguist-generated
.cursor/rules/**          linguist-generated
.github/copilot-instructions.md linguist-generated
```

Adjust the paths for the targets you configure. These entries affect only GitHub's diff display. They do not change Git tracking or prevent tools from reading the files.
