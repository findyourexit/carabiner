# Carabiner MCP Server

Carabiner includes a Model Context Protocol server for managing canonical Carabiner files in the current workspace. The server communicates over stdio and exposes one tool named `carabinerTool`.

## Install

Install the CLI before registering or starting the server:

```bash
cargo install carabiner --locked
```

## Start and configure the server

Start the server from the workspace whose canonical files it should manage:

```bash
carabiner mcp
```

The server reads JSON-RPC messages from standard input and writes JSON-RPC responses to standard output. Its file paths are relative to the process working directory.

### Register the server

Add the server to the canonical MCP configuration at `.carabiner/mcp.jsonc`. Carabiner can then generate the selected target's native MCP configuration from this source file.

```json title=".carabiner/mcp.jsonc"
{
  "$schema": "https://github.com/findyourexit/carabiner/releases/latest/download/mcp-schema.json",
  "mcpServers": {
    "carabiner-mcp": {
      "type": "stdio",
      "command": "carabiner",
      "args": ["mcp"],
      "env": {}
    }
  }
}
```

The `carabiner-mcp` key is the MCP server name. The process that launches the server must use the project root as its working directory.

## `carabinerTool`

`carabinerTool` multiplexes all operations through the required `feature` and `operation` fields. It is the only tool returned by `tools/list`.

| Feature | Supported operations |
| --- | --- |
| `rule`, `command`, `subagent`, `skill`, `check` | `list`, `get`, `put`, `delete` |
| `ignore`, `mcp`, `permissions`, `hooks` | `get`, `put`, `delete` |
| `generate`, `import`, `convert` | `run` |

`list` calls do not take a path. For the Markdown features, `get`, `put`, and `delete` require `targetPathFromCwd`. A Markdown `put` also requires a nonempty `body`. Its `frontmatter` is optional. For singleton configuration features, `put` requires a nonempty `content` string.

## Canonical file operations

### Markdown features

Carabiner stores the Markdown features at these canonical paths:

| Feature | Canonical path |
| --- | --- |
| `rule` | `.carabiner/rules/<file>.md` |
| `command` | `.carabiner/commands/<file>.md` |
| `subagent` | `.carabiner/subagents/<file>.md` |
| `skill` | `.carabiner/skills/<skill>/SKILL.md` |
| `check` | `.carabiner/checks/<file>.md` |

For `rule`, `command`, `subagent`, and `check`, the path must name a nonhidden Markdown file. Carabiner uses its filename in the canonical directory for that feature. A skill path must be a directory inside `.carabiner/skills` or that directory's `SKILL.md` file.

Markdown `get` responses contain the canonical relative path, parsed `frontmatter`, and `body`. `list` responses contain each item path and frontmatter. Skill responses and listings use `relativeDirPathFromCwd` for the skill directory instead of the path to `SKILL.md`.

On `put`, Carabiner writes a YAML frontmatter block followed by the supplied body. When omitted, `frontmatter.targets` defaults to `["*"]`. If supplied, `targets` must be an array of `"*"` or valid target names. `description` must be a string, `root` and `localRoot` must be booleans, and `globs` must be an array of strings. A subagent requires `frontmatter.name`. A skill requires both `frontmatter.name` and `frontmatter.description`. A check can set `frontmatter.severity` to `low`, `medium`, `high`, or `critical`, and `frontmatter.tools` must be an array of strings.

### Singleton configuration features

The singleton features manage one configuration file at a time. When more than one compatible file exists, Carabiner selects the first existing path in the order shown below. If none exists, `put` uses the first path.

| Feature | Path candidates |
| --- | --- |
| `ignore` | `.carabiner/.aiignore`, `.carabinerignore` |
| `mcp` | `.carabiner/mcp.jsonc`, `.carabiner/mcp.json`, `.carabiner/.mcp.json` |
| `permissions` | `.carabiner/permissions.jsonc`, `.carabiner/permissions.json` |
| `hooks` | `.carabiner/hooks.jsonc`, `.carabiner/hooks.json` |

The `ignore` feature accepts text content. The `mcp`, `permissions`, and `hooks` features require `content` to be valid JSONC whose root value is an object. `delete` removes every existing compatible path for that feature.

### Skill files other than `SKILL.md`

A skill can contain files in addition to `SKILL.md`. Pass them through `otherFiles` on a skill `put`. Each entry is relative to the skill directory.

| Field | Type | Required | Description |
| --- | --- | --- | --- |
| `name` | `string` | Yes | Relative file path, such as `references/logo.png`. It cannot be `SKILL.md`. |
| `body` | `string` | Yes | File content encoded according to `encoding`. |
| `encoding` | `"utf-8"` or `"base64"` | No | Defaults to `"utf-8"`. Use `"base64"` for binary data. |

On `get`, each other file has an explicit `encoding`. Carabiner returns valid UTF-8 content as `"utf-8"` and other bytes as `"base64"`. Preserve that field when sending a response back to `put`. Sending a base64 body without its `encoding` stores the base64 text rather than the original bytes.

Base64 content can use the standard or URL-safe alphabet with optional padding. ASCII whitespace is ignored, and the encoded value must be canonical. Each supplied other file is limited to 1 MB after decoding. The complete skill, including its frontmatter, body, and other files, is also limited to 1 MB. Files not included in an `otherFiles` `put` request are left unchanged.

## `run` operations

Use `operation: "run"` for generation, import, and conversion. Each feature accepts the option object described below.

### Generate

`feature: "generate"` accepts an optional `generateOptions` object.

| Option | Type | Description |
| --- | --- | --- |
| `targets` | `string[]` | Target names to generate. |
| `features` | `string[]` | Canonical features to generate. |
| `delete` | `boolean` | Delete generated files that no longer belong to the resolved configuration. |
| `global` | `boolean` | Generate user-scope configuration. |
| `simulateCommands` | `boolean` | Generate simulated command output where supported. |
| `simulateSubagents` | `boolean` | Generate simulated subagent output where supported. |
| `simulateSkills` | `boolean` | Generate simulated skill output where supported. |

### Import

`feature: "import"` requires an `importOptions` object.

| Option | Type | Required | Description |
| --- | --- | --- | --- |
| `target` | `string` | Yes | Target tool to import from. It must be a valid target name. |
| `features` | `string[]` | No | Features to import. |
| `global` | `boolean` | No | Import user-scope configuration. |

### Convert

`feature: "convert"` requires a `convertOptions` object.

| Option | Type | Required | Description |
| --- | --- | --- | --- |
| `from` | `string` | Yes | Source tool name. It must be a valid target name. |
| `to` | `string[]` | Yes | Destination tool names. The array cannot be empty and cannot include `from`. |
| `features` | `string[]` | No | Features to convert. Omit it to convert all features. |
| `global` | `boolean` | No | Convert user-scope configuration. |
| `dryRun` | `boolean` | No | Report the conversion without writing files. |

Conversion rejects `antigravity-plugin` and `claudecode-plugin` as either source or destination targets.

## Protocol behavior

The server implements `initialize`, `tools/list`, `tools/call`, and `ping`, plus the `notifications/initialized` and `notifications/cancelled` notifications. During initialization it advertises MCP protocol version `2024-11-05`. A successful tool call returns formatted JSON in a text content item. Validation failures use JSON-RPC error responses, while operational failures from `generate`, `import`, and `convert` return a JSON result with `success: false` and an `error` message.
