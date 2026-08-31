# Quick Start

## Start a New Project

Install Carabiner and create a project directory. The `init` command creates the `.carabiner/` source directory and a `carabiner.jsonc` configuration file when they do not already exist.

```bash
cargo install carabiner --locked
mkdir my-project
cd my-project
carabiner init
```

Edit the source files in `.carabiner/`, then generate configuration for the AI coding tools you use.

```bash
carabiner generate --targets "claudecode,cursor,copilot" --features "rules,mcp,commands,subagents,skills"
```

## Add Official Skills

Fetch the official skill collection into `.carabiner/`. By default, `fetch` requests skills and writes the fetched files to that directory.

```bash
carabiner fetch findyourexit/carabiner
```

## Import Existing Configuration

If a project already has configuration for an AI coding tool, import it into `.carabiner/` before generating new files.

```bash
carabiner import --targets claudecode
carabiner import --targets cursor
carabiner import --targets copilot
carabiner import --targets claudecode --features rules,mcp,commands,subagents
```

For every command and option, see [CLI Commands](/reference/cli-commands).
