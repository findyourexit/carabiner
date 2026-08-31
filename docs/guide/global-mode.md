# Global Mode

Global mode writes configuration to a selected target's user-level location instead of a project directory. Keep its source files in a directory that is separate from individual projects.

## Set Up a Global Source Directory

Create a directory for the source files and initialize it.

```bash
mkdir -p ~/.carabiner-global
cd ~/.carabiner-global
carabiner init
```

## Enable Global Mode

Set `global` to `true` in `carabiner.jsonc`. Select explicit targets and features so that only the configuration you intend to manage is generated.

```jsonc title="carabiner.jsonc"
{
  "global": true,
  "targets": ["claudecode"],
  "features": ["rules", "commands", "subagents", "skills"]
}
```

## Add Source Rules

Global mode uses the same `.carabiner/` source layout as project mode. For example, add `.carabiner/rules/overview.md`.

```md title=".carabiner/rules/overview.md"
---
root: true
targets: ["claudecode"]
---

# Personal Coding Guidance

Follow the coding conventions that apply to every project.
```

## Generate Global Configuration

Run generation from the global source directory.

```bash
carabiner generate
```

Carabiner writes beneath your home directory using the selected target's global paths. Supported features and paths vary by target, so use explicit targets and features when you need to limit output.

Preview the changes before writing them.

```bash
carabiner generate --dry-run
```

## Use Global Mode for One Command

You can enable global mode without changing `carabiner.jsonc`.

```bash
carabiner generate --global --targets claudecode --features rules
```
