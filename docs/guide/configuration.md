# Configuration

Carabiner reads `carabiner.jsonc` from the root of a project. The file uses JSONC, so it can contain comments and trailing commas. Use it to select targets and features and to control generation.

## JSON Schema

Carabiner provides a JSON Schema for editor validation and completion. Add `$schema` to `carabiner.jsonc`:

```jsonc title="carabiner.jsonc"
{
  "$schema": "https://github.com/findyourexit/carabiner/releases/latest/download/config-schema.json",
  "targets": ["claudecode"],
  "features": ["rules"],
}
```

## Project Configuration

Use the array form of `targets` when every selected target should receive the same features:

```jsonc title="carabiner.jsonc"
{
  "$schema": "https://github.com/findyourexit/carabiner/releases/latest/download/config-schema.json",

  // Targets to generate. Use "*" for all supported non-legacy targets.
  "targets": ["cursor", "claudecode", "opencode", "codexcli"],

  // Features to generate. Use "*" for all features.
  "features": ["rules", "mcp", "commands", "subagents", "hooks", "permissions"],

  // Directories that receive generated files. Most projects use ["."].
  // A monorepo can list multiple roots when each package needs generated configuration.
  "outputRoots": ["."],

  // Remove existing generated files before writing.
  "delete": true,

  // Print detailed output.
  "verbose": false,

  // Suppress normal output. Errors are still reported.
  "silent": false,

  // Advanced generation options.
  "global": false, // Generate user-scope configuration files.
  "simulateCommands": false, // Generate simulated commands.
  "simulateSubagents": false, // Generate simulated subagents.
  "simulateSkills": false, // Generate simulated skills.

  // Choose names for command files generated for tools without command subdirectory support.
  "flattenedCommandNaming": "basename",

  // Limit `carabiner gitignore` entries to the configured targets.
  "gitignoreTargetsOnly": true,

  // Declarative rule and skill sources installed with `carabiner install`.
  // See the Declarative Sources guide for details.
  // "sources": [
  //   { "source": "owner/repo" },
  //   { "source": "org/repo", "skills": ["specific-skill"] },
  //   { "source": "org/standards", "rules": ["testing-guidelines"] },
  // ],
}
```

### Flattened command file names

Some tools do not support command subdirectories. For those targets, `flattenedCommandNaming` determines how a command source path becomes a generated file name.

- `"basename"` is the default. It keeps only the file name, so `pj/test.md` and `ops/test.md` both become `test.md`. The later generated file replaces the earlier one.
- `"path"` joins directory segments into the file name, so `pj/test.md` becomes `pj-test.md`. It reduces collisions but cannot prevent them because a source file already named `pj-test.md` also becomes `pj-test.md`.

Targets that support command subdirectories, including Claude Code, are unaffected. After changing this option, run `carabiner generate --delete` once. You can instead set `"delete": true` and run `carabiner generate`. This removes stale files that use the previous naming mode.

### Gitignore target selection

When `gitignoreTargetsOnly` is `true`, which is the default, `carabiner gitignore` emits entries only for the tools listed in `targets`. Set it to `false` to emit entries for every supported tool.

Unless you pass an explicit `--targets` option, Carabiner adds `agentsmd` entries for `AGENTS.md` and related paths whenever a configuration file exists and `agentsmd` is not already selected. Many AI tools read those conventional files, so the entries help prevent generated rules from being committed by mistake.

## Per-Target Features

The `targets` option accepts either an array or an object. Use an object to choose features separately for each target:

=== "Array form"

    ```jsonc title="carabiner.jsonc"
    {
      "targets": {
        "claudecode": ["rules", "commands"],
        "cursor": ["rules", "mcp"],
        "copilot": ["rules", "subagents"],
      },
    }
    ```

    This configuration generates rules and commands for `claudecode`, rules and MCP configuration for `cursor`, and rules and subagents for `copilot`.

    !!! warning
        When `targets` uses the object form, omit the top-level `features` field. The configuration loader rejects a configuration that defines both.

    Use `"*"` inside a target's feature array to enable every feature for that target:

    ```jsonc title="carabiner.jsonc"
    {
      "targets": {
        "claudecode": ["*"], // Generate all features for Claude Code.
        "cursor": ["rules"], // Generate only rules for Cursor.
      },
    }
    ```

=== "Object form"

    ### Per-feature options

    Use an object instead of an array for a target's value when a feature needs options. Each feature key can be `true`, `false`, or an options object.

    ```jsonc title="carabiner.jsonc"
    {
      "gitignoreDestination": "gitignore",
      "targets": {
        "claudecode": {
          "gitignoreDestination": "gitattributes",
          "rules": { "ruleDiscoveryMode": "explicit" },
          "ignore": {
            "fileMode": "local",
            "gitignoreDestination": "gitignore",
          },
        },
      },
    }
    ```

    `gitignoreDestination` selects where `carabiner gitignore` writes path entries. You can set it at the root, target, or target-feature level. The allowed values are `"gitignore"`, which is the default, and `"gitattributes"`.

    A `"gitattributes"` setting at target-feature level takes precedence over one at target level. A target-level `"gitattributes"` setting takes precedence over the root setting. When neither scope selects `"gitattributes"`, Carabiner uses the root setting.

    The available per-feature options are:

    | Target | Feature | Option | Values | Default |
    | --- | --- | --- | --- |
    | `claudecode` | `rules` | `ruleDiscoveryMode` | `"none"` or `"explicit"` | Target default |
    | Any target | `rules` | `includeLocalRoot` | `true` or `false`. When `false`, rules marked `localRoot` are skipped for that target. | `true` |
    | `claudecode` | `ignore` | `fileMode` | `"shared"` for `settings.json` or `"local"` for `settings.local.json` | `"shared"` |
    | Any target | Any feature | `gitignoreDestination` | `"gitignore"` or `"gitattributes"` | `"gitignore"` |

    See [`docs/reference/file-formats.md`](../reference/file-formats.md#where-ignore-patterns-are-written-per-tool) for the Claude Code `fileMode` default and guidance on using `"local"`.

## Local Configuration

Carabiner also loads `carabiner.local.jsonc` from the same directory as `carabiner.jsonc`. Use it for machine-specific or developer-specific settings. `carabiner gitignore` adds it to `.gitignore`, so it should not be committed.

Configuration values are resolved in this order, from highest to lowest priority:

1. CLI options
2. `carabiner.local.jsonc`
3. `carabiner.jsonc`
4. Default values

For example, a developer can select a local target and enable detailed output without changing the shared configuration:

```jsonc title="carabiner.local.jsonc"
{
  "$schema": "https://github.com/findyourexit/carabiner/releases/latest/download/config-schema.json",
  // Override targets for local development.
  "targets": ["claudecode"],
  // Enable detailed output for debugging.
  "verbose": true,
}
```

## Target Order and File Conflicts

When multiple targets write the same output file, the last target in the `targets` array wins. For example, both `agentsmd` and `opencode` generate `AGENTS.md`:

```jsonc title="carabiner.jsonc"
{
  // opencode wins because it comes last.
  "targets": ["agentsmd", "opencode"],
  "features": ["rules"],
}
```

Carabiner generates `AGENTS.md` for `agentsmd` first, then generates it for `opencode`, replacing the previous file. Reverse the order when `agentsmd` should supply the final file:

```jsonc title="carabiner.jsonc"
{
  // agentsmd wins because it comes last.
  "targets": ["opencode", "agentsmd"],
  "features": ["rules"],
}
```
