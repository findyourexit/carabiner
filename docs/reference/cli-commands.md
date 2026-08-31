# CLI Command Reference

Install Carabiner with `cargo install carabiner --locked`.

## Quick Commands

=== "Scaffold & import"
    ```bash
    # Initialize a project with an organized rules structure
    carabiner init

    # Import an existing configuration into .carabiner/rules/ by default
    carabiner import --targets claudecode --features rules,mcp,commands,subagents,skills,permissions

    # Import components from an existing plugin directory
    carabiner import --targets claudecode-plugin --features skills,hooks --output-root ./plugins/review-tools
    ```

=== "Generate"
    ```bash
    # Generate every feature for every tool
    carabiner generate --targets "*" --features "*"

    # Generate selected features for selected tools
    carabiner generate --targets copilot,cursor,cline --features rules,mcp
    carabiner generate --targets claudecode --features rules,subagents

    # Generate components in an existing plugin directory
    carabiner generate --targets antigravity-plugin --features rules,mcp,subagents,skills,hooks --output-roots ./plugins/review-tools

    # Generate rules only
    carabiner generate --targets "*" --features rules

    # Generate simulated commands and subagents
    carabiner generate --targets copilot,cursor,codexcli --features commands,subagents --simulate-commands --simulate-subagents

    # Preview changes without writing files
    carabiner generate --dry-run --targets claudecode --features rules

    # Check whether generated files are current for continuous integration
    carabiner generate --check --targets "*" --features "*"

    # Generate from a shared source tree without changing directories
    carabiner generate --input-roots ~/.aiglobal/.carabiner --targets "*" --features rules
    ```

=== "Sources"
    ```bash
    # Fetch configuration from a Git repository
    carabiner fetch owner/repo
    carabiner fetch owner/repo@v1.0.0 --features rules,commands
    carabiner fetch https://github.com/owner/repo --conflict skip

    # Install rules and skills declared in carabiner.jsonc
    carabiner install

    # Add a source to carabiner.jsonc, update its lockfile, and install it
    carabiner add anthropics/skills --skills skill-creator

    # Add a rule source without selecting skills
    carabiner add acme/ai-standards --rules testing-guidelines

    # Resolve every source reference again and ignore the lockfile
    carabiner install --update

    # Require an up-to-date lockfile and fetch artifacts by locked reference
    carabiner install --frozen

    # Install sources and then generate configuration
    carabiner install && carabiner generate
    ```

=== "Utilities"
    ```bash
    # Convert one tool's configuration to other tools without writing .carabiner/
    carabiner convert --from cursor --to copilot,claudecode
    carabiner convert --from cursor --to copilot,claudecode --features rules,mcp

    # Add generated files to .gitignore
    carabiner gitignore

    # Add entries for selected tools
    carabiner gitignore --targets claudecode,copilot

    # Add entries for selected features
    carabiner gitignore --targets copilot --features rules,commands

    # Diagnose configuration without writing files
    carabiner doctor

    # Treat warnings as failures
    carabiner doctor --strict

    # Print GitHub release notes for the Carabiner repository
    carabiner release-notes findyourexit/carabiner

    # Print the five most recent releases
    carabiner release-notes findyourexit/carabiner --latest 5

    # Update a Carabiner release. A repository is required.
    carabiner update --repository owner/carabiner

    # Check for updates without installing one
    carabiner update --repository owner/carabiner --check

    # Update even when the installed version is already current
    carabiner update --repository owner/carabiner --force
    ```

??? warning "Deprecated"
    Existing projects can continue to use `ignore`, but new projects should use `permissions`. Any removal will be decided separately and will not happen before a future major release.

## Generate Command

`generate` reads source files from one or more managed source trees and writes configuration files for AI tools. By default, it reads `<cwd>/.carabiner`. Use `--input-roots` to choose different source trees.

### Options

| Option | Description | Default |
| --- | --- | --- |
| `--targets, -t <tools>` | Comma-separated tools, such as `claudecode,copilot` or `*`. | From `carabiner.jsonc` |
| `--features, -f <features>` | Comma-separated features: rules, commands, subagents, skills, mcp, hooks, permissions, checks, and deprecated ignore. | From `carabiner.jsonc` |
| `--input-roots <paths...>` | Ordered source-tree directories, such as `.carabiner` and `.carabiner.local`. Each value names a source tree directly, so Carabiner does not append `.carabiner/`. The first root must exist. Later roots are optional overlays and can be absent. Later roots override earlier roots for the same relative source path. This option applies to `generate` only and cannot be used with `--input-root`. | `<cwd>/.carabiner` |
| `--input-root <path>` | **Deprecated.** Parent directory of a `.carabiner/` source tree. Carabiner expands it to `--input-roots <path>/.carabiner`. Use `--input-roots` instead. This option cannot be used with `--input-roots`. | Current directory |
| `--dry-run` | Show planned changes without writing files. | `false` |
| `--check` | Behaves like `--dry-run` and exits with code `1` when files are not current. | `false` |
| `--global` | Generate user-scope configuration files. | `false` |
| `--simulate-commands` | Generate simulated commands for tools without native command support. | `false` |
| `--simulate-subagents` | Generate simulated subagents for tools without native subagent support. | `false` |
| `--simulate-skills` | Generate simulated skills for tools without native skill support. | `false` |
| `--delete` | Delete existing generated files before writing. | From `carabiner.jsonc` |
| `--watch, -w` | Keep running and regenerate when source files change. | `false` |

!!! info "Shared output directories"
    Some targets intentionally write to the same directories, including `.agents/agents/`, `.agents/skills/`, and other cross-vendor roots. The orphan sweep runs only after every target and feature finishes writing. It never removes a path written during the current run, so one target cannot remove a sibling's fresh output. A synchronized tree produces no changes under `--check`. The sweep removes only files in generated directories that no `.carabiner/` source produces.

### Examples

```bash
# Generate every feature for every configured tool
carabiner generate

# Generate rules for every tool
carabiner generate --targets "*" --features rules

# Generate from a shared source tree without changing directories
carabiner generate --input-roots ~/.aiglobal/.carabiner --targets "*" --features rules

# Preview changes without writing files
carabiner generate --dry-run --targets claudecode --features rules

# Fail if generated files are not current
carabiner generate --check --targets "*" --features "*"

# Regenerate when source files change
carabiner generate --watch
```

### Watch Mode

`generate --watch` generates once, then watches the managed source files and generates again when they change. It is useful when editing rules, commands, subagents, or skills.

- **Watched paths:** Carabiner watches the `.carabiner/` tree recursively and the adjacent `carabiner.jsonc` and `carabiner.local.jsonc` files. It watches the file passed through `--config` instead when that option is used. Generated output is never watched, so a generation cannot trigger another generation.
- **Debouncing:** A burst of file-system events, such as an editor save sequence or `git checkout`, becomes one generation after a short quiet period. A change during generation schedules exactly one follow-up generation.
- **Errors:** A failed generation, such as one caused by invalid frontmatter saved during editing, is reported without stopping the watcher.
- **Configuration changes:** Editing the configuration file starts a generation that uses the newly resolved configuration. The watched paths are chosen at startup. Changing `inputRoot`, `inputRoots`, or the configuration file location requires restarting the command. Carabiner prints a warning when the configuration file changes.
- **Incompatible flags:** `--watch` cannot be combined with `--check`, `--dry-run`, or `--json`. The first two verify a single run, and `--json` emits one result document when a command exits.
- **Stopping:** `Ctrl+C` (`SIGINT`) and `SIGTERM` close the watchers and exit normally.

### Global Profile Overrides

Hermes Agent (`HERMES_HOME`) and Kimi Code (`KIMI_CODE_HOME`) use environment variables to choose their profile locations. When either variable is set, `generate --global` and `convert --global` write that target's output under the specified directory. This takes precedence over both `outputRoots` and an explicit `--output-roots` value for that target. See [Supported Tools](./supported-tools.md) for each profile root's contents. The behavior is intentional because these variables identify the locations those tools read. Other targets continue to use their configured output roots.

The environment variable must name a usable directory. An empty value is ignored and the default profile location is used. A filesystem root or an unnormalized path is rejected with an error that names the variable.

### Shared Configuration Files

Carabiner merges some output files rather than owning them because a tool or user can keep unrelated settings in the same file. These files are `.amp/settings.json(c)`, `.antigravity/settings.json`, `.claude/settings.json`, `.claude/settings.local.json`, `.codex/config.toml`, `.copilot/settings.json`, `.devin/config.json`, `.factory/settings.json`, `.github/copilot/settings.json`, `.grok/config.toml`, `.vibe/config.toml`, `.vscode/settings.json`, `.zed/settings.json`, `kilo.json(c)`, `opencode.json(c)`, and `reasonix.toml`. `carabiner gitignore` deliberately does not add them to `.gitignore`, which lets hand-authored settings stay under version control.

Because these files can be committed, `generate` does not create one solely for an empty payload. If Carabiner has nothing to add, such as when no permissions map to a tool, the file remains absent instead of being written as `{}`. An existing file is still rewritten normally, so hand-authored settings are not removed. Every other generated file is written even when empty because its existence is part of Carabiner's output.

## Gitignore Command

`gitignore` adds generated AI tool configuration files to `.gitignore`. By default, it adds entries only for tools listed in the `targets` field of `carabiner.jsonc`. The `gitignoreTargetsOnly` option controls this behavior and defaults to `true`. Set it to `false` to add entries for every supported tool. Per-run `--targets` and `--features` filters take precedence over the configuration.

Set `gitignoreDestination` to `"gitattributes"` at the root, tool, or tool and feature level to write entries to `.gitattributes` instead. More specific values take precedence.

!!! note "No `carabiner.jsonc` file?"
    Carabiner adds entries for every supported tool. `gitignoreTargetsOnly` applies only when a configuration file exists, so projects without one still receive useful `.gitignore` coverage.

!!! warning "`agentsmd` entries are always included"
    When `gitignoreTargetsOnly` is `true` and `agentsmd` is absent from `targets`, entries for `AGENTS.md` and related paths are still added. `AGENTS.md` is a common file that many AI tools read without regard to the selected targets. To omit these entries, pass an explicit `--targets` value that does not include `agentsmd`.

### Options

| Option | Description | Default |
| --- | --- | --- |
| `--targets, -t <tools>` | Comma-separated tools to include, such as `claudecode,copilot` or `*` for all tools. | Derived from `targets` and `gitignoreTargetsOnly` |
| `--features, -f <features>` | Comma-separated features: rules, commands, subagents, skills, ignore, mcp, hooks, permissions, and checks. | `*` |

### Examples

```bash
# Add all entries
carabiner gitignore

# Add entries for Claude Code
carabiner gitignore --targets claudecode

# Add entries for several tools
carabiner gitignore --targets claudecode,copilot,cursor

# Add only rule and command entries for Copilot
carabiner gitignore --targets copilot --features rules,commands
```

### Behavior

- **Common entries:** Paths such as `.carabiner/rules/.curated/`, `.carabiner/skills/.curated/`, and `carabiner.local.jsonc` are always included, regardless of filters.
- **General entries:** Entries for general paths, such as memories and settings, are included when their target is selected.
- **Repeated runs:** A new run removes previously generated entries before writing the selected set.

## Add Command

`add` can create a starter file for one Carabiner feature or append a declarative source to `carabiner.jsonc`.

### Feature Scaffolding

Use a feature keyword to create a valid starter file that you can edit:

```bash
# Named Markdown features
carabiner add rule --name overview
carabiner add command --name review-pr.md
carabiner add subagent --name planner
carabiner add skill --name project-context
carabiner add check --name security

# Singleton features
carabiner add mcp
carabiner add hooks
carabiner add permissions

# Deprecated compatibility scaffold. Prefer permissions.
carabiner add ignore
```

Named features accept names with or without the `.md` suffix. Skills use `.carabiner/skills/<name>/SKILL.md`. The other named features create `<name>.md` in their canonical source directories. Names cannot contain path separators.

When a target file already exists, an interactive invocation asks before replacing it. Declining leaves the file unchanged. JSON, silent, and noninteractive invocations fail safely. Pass `--force` to replace a file explicitly. Singleton scaffolds recognize supported JSONC and legacy variants and replace the effective existing file rather than creating a shadowed canonical file.

Feature keywords are reserved when no source-specific option is supplied. To add a source whose identifier is a feature keyword, provide an option that makes the source intent clear. For example, use `carabiner add skill --transport npm`.

### Declarative Sources

For any other source identifier, `add` appends one source to `carabiner.jsonc` and runs the declarative source resolver immediately. It preserves JSONC comments, installs selected rules in `.carabiner/rules/.curated/`, installs selected skills in `.carabiner/skills/.curated/`, and updates `carabiner.lock` or `carabiner-npm.lock.json`.

```bash
# GitHub source, the default transport
carabiner add anthropics/skills --skills skill-creator

# Rules only. Direct Markdown files are selected from rules/.
carabiner add acme/ai-standards --rules testing-guidelines,typescript-conventions

# Rules and skills from separate paths in one source
carabiner add acme/ai-assets --rules "*" --rules-path exports/rules --skills review-pr --path exports/skills

# Any Git remote through the git command-line client
carabiner add https://example.com/team/skills.git --transport git --ref main --path skills

# An npm-compatible registry
carabiner add @acme/skill-package --transport npm --registry https://registry.npmjs.org
```

The selected configuration file must already exist. Run `carabiner init` first or pass `--config <path>`. Adding a source with an existing normalized identity fails rather than creating duplicate lockfile entries. Edit the existing declaration when its options need to change.

`add` fetches only the source being added. Existing declarations are not fetched again. Existing sources must already be locked and installed, so run `carabiner install` first when needed. The operation is transactional. If the new source cannot be installed, Carabiner restores the manifest, source lockfiles, curated rules, and curated skills to their previous state.

| Option | Description |
| --- | --- |
| `--name <name>` | Name of a rule, command, subagent, skill, or check scaffold. |
| `--force` | Replace an existing scaffold file without prompting. |
| `--skills <skills>` | Comma-separated skill names. `*` selects every skill. |
| `--rules <rules>` | Comma-separated rule names. Names can omit `.md`. `*` selects direct Markdown files under `rulesPath`. |
| `--transport <type>` | `github`, which is the default, `git`, or experimental `npm`. |
| `--ref <ref>` | Git reference, npm version, or npm distribution tag. |
| `--path <path>` | Skill path within the source. Defaults to `skills`. |
| `--rules-path <path>` | Rule path within the source. Defaults to `rules`. |
| `--registry <url>` | npm-compatible registry URL. |
| `--token-env <name>` | Environment variable that holds the npm registry token. |
| `--token <token>` | GitHub token for private repositories. |
| `--config <path>` | Configuration file to edit. Defaults to `carabiner.jsonc`. |

When neither `--skills` nor `--rules` is supplied, every skill is installed for backward compatibility. Supplying only `--rules` installs no skills.

## Fetch Command

`fetch` copies configuration files directly from GitHub repositories. GitLab support is planned.

!!! note
    This feature is still in development and may change in future releases.

`fetch` looks for feature directories such as `rules/`, `commands/`, `skills/`, and `subagents/` at the chosen repository path. It does not require a `.carabiner/` directory, so it can read external repositories such as `vercel-labs/agent-skills` and `anthropics/skills`.

### Source Formats

```bash
# Full URL
carabiner fetch https://github.com/owner/repo
carabiner fetch https://github.com/owner/repo/tree/branch
carabiner fetch https://github.com/owner/repo/tree/branch/path/to/subdir
carabiner fetch https://gitlab.com/owner/repo  # GitLab support is planned

# Provider prefix
carabiner fetch github:owner/repo
carabiner fetch gitlab:owner/repo              # GitLab support is planned

# GitHub shorthand
carabiner fetch owner/repo
carabiner fetch owner/repo@ref        # Branch, tag, or commit
carabiner fetch owner/repo:path       # Subdirectory
carabiner fetch owner/repo@ref:path   # Reference and subdirectory
```

### Options

| Option | Description | Default |
| --- | --- | --- |
| `--target, -t <target>` | Format used to interpret fetched files, such as the internal `carabiner` format or `claudecode`. | `carabiner` |
| `--features, -f <features>` | Comma-separated features: rules, commands, subagents, skills, ignore, mcp, hooks, permissions, and checks. | `skills` |
| `--output, -o <dir>` | Output directory relative to the project root. | `.carabiner` |
| `--conflict, -c <strategy>` | Conflict strategy: `overwrite` or `skip`. | `overwrite` |
| `--ref, -r <ref>` | Git reference to fetch, such as a branch, tag, or commit. | Default branch |
| `--path, -p <path>` | Repository subdirectory to fetch. | Repository root |
| `--skills <skills>` | Comma-separated skill names to fetch. Requires the skills feature. | All skills |
| `--interactive, -i` | Select skills through an interactive prompt. Requires the skills feature and a terminal. | Disabled |
| `--token <token>` | Git provider token for private repositories. | `GITHUB_TOKEN` or `GH_TOKEN` |

### Examples

```bash
# Fetch skills from external repositories
carabiner fetch vercel-labs/agent-skills
carabiner fetch anthropics/skills

# Fetch selected skills
carabiner fetch anthropics/skills --skills pdf,docx

# Select skills interactively
carabiner fetch anthropics/skills --interactive

# Select skills interactively with pdf selected initially
carabiner fetch anthropics/skills --interactive --skills pdf

# Fetch every feature from a public repository
carabiner fetch findyourexit/carabiner --path .carabiner --features "*"

# Fetch rules and commands from a tag
carabiner fetch owner/repo@v1.0.0 --features rules,commands

# Fetch from a private repository with GITHUB_TOKEN
export GITHUB_TOKEN=ghp_xxxx
carabiner fetch owner/private-repo

# Use GitHub CLI to provide the token
GITHUB_TOKEN=$(gh auth token) carabiner fetch owner/private-repo

# Keep existing files on conflict
carabiner fetch owner/repo --conflict skip

# Fetch from a monorepo subdirectory
carabiner fetch owner/repo:packages/my-package
```

## Convert Command

`convert` translates configuration from one AI tool directly to one or more destination tools without creating `.carabiner/` files on disk. It keeps the intermediate canonical representation in memory.

Use this command for a one-time tool-to-tool conversion, such as translating Cursor rules to Claude Code and Copilot equivalents, without adopting a managed source-tree workflow.

### Options

| Option | Description | Default |
| --- | --- | --- |
| `--from <tool>` | Source tool. Only one tool is allowed. | Required |
| `--to <tools>` | Comma-separated destination tools, such as `copilot,claudecode`. | Required |
| `--features, -f <features>` | Comma-separated features to convert: rules, commands, subagents, skills, ignore, mcp, hooks, permissions, and checks. | `*` |
| `--verbose, -V` | Print detailed output. | `false` |
| `--silent, -s` | Suppress output. | `false` |
| `--global, -g` | Convert user-scope configuration files. | `false` |
| `--dry-run` | Show changes without writing files. | `false` |

### Examples

```bash
# Convert Cursor rules to Copilot and Claude Code
carabiner convert --from cursor --to copilot,claudecode --features rules

# Convert all mutually supported features from Cursor to Copilot
carabiner convert --from cursor --to copilot

# Convert MCP configuration from Claude Code to Cursor
carabiner convert --from claudecode --to cursor --features mcp

# Preview a conversion
carabiner convert --from cursor --to copilot,claudecode --dry-run
```

### Behavior

- Intermediate canonical files are never written to disk. Only destination tool files are written.
- A feature supported by the source but not by a destination is skipped with a warning.
- Without `--features`, Carabiner attempts every feature supported by the source tool.
- A source tool in `--to` is rejected because converting a tool onto itself is lossy.
- With `--dry-run`, no destination file is written. Carabiner prints a `[DRY RUN]` summary of the planned conversion.

## Doctor Command

`doctor` performs read-only diagnostics on `carabiner.jsonc` and `carabiner.local.jsonc`. It groups findings as `error`, `warning`, or `info` and never writes files. Use it when generation does not behave as expected or as a continuous integration check.

It can identify silently ignored configuration. The schema accepts unknown keys, so a misspelling such as `"target"` instead of `"targets"` normally causes no error. `doctor` reports each unknown key with a suggestion.

### Checks

- JSONC parse errors with line and column numbers.
- Unknown or misspelled top-level keys with a suggestion.
- Unknown tool targets and features in array and object forms with the nearest valid name.
- Deprecated `ignore` features that should be replaced with `permissions`.
- Object-form `targets` used with `features`, including conflicts that appear only after merging `carabiner.jsonc` and `carabiner.local.jsonc`.
- Conflicting target pairs, such as `claudecode` and `claudecode-legacy`.
- Whether `$schema` is present and points to the current configuration schema URL.
- Structural schema violations in other keys, such as incorrect types or malformed `sources` entries.
- A `sources[].tokenEnv` value that names an unset environment variable.
- An `inputRoot` value or first `inputRoots` entry that does not name an existing directory. Later `inputRoots` entries are optional overlays and can be absent.
- An `inputRoot` or `inputRoots` entry that is empty. This causes `generate` to fail with `outputRoot cannot be an empty string` before resolving a source tree.
- Duplicate `inputRoots` entries. Duplicates are ignored by `generate`.

### Options

| Option | Description | Default |
| --- | --- | --- |
| `--config, -c <path>` | Configuration file to diagnose. | `carabiner.jsonc` |
| `--strict` | Treat warnings as errors and exit with code `1`. | `false` |
| `--verbose, -V` | Print detailed output. | `false` |
| `--silent, -s` | Suppress output. | `false` |

### Examples

```bash
# Diagnose the project configuration
carabiner doctor

# Fail on warnings too
carabiner doctor --strict

# Emit output for editors and continuous integration
carabiner --json doctor

# Diagnose a configuration file at a custom path
carabiner doctor --config ./configs/carabiner.jsonc
```

### Behavior

- `doctor` exits with code `1` for an `error` finding or for a `warning` finding with `--strict`. Otherwise, it exits with code `0`.
- With the global `--json` option, diagnostics and a severity summary are structured JSON. Successful output is in `data`. Failure output is in `error.details` and uses the `DOCTOR_FAILED` code.
- A missing configuration file is reported as `info`. Carabiner uses its built-in defaults.

## Docs Command

`docs` prints documentation bundled with Carabiner to standard output. The documentation is embedded in the compiled CLI, so it is available in the installed binary without browsing a repository or website.

Document identifiers follow the `docs/` hierarchy without the leading `docs/` directory or the `.md` extension. Both forms are accepted. Absolute paths, drive letters, and `..` segments are rejected so an identifier cannot leave the bundled document tree.

### Usage

```bash
# List every document identifier
carabiner docs

# Print a top-level or nested document
carabiner docs faq
carabiner docs guide/configuration

# Search the bundled documentation
carabiner docs --search "global mode"
```

### Search

`--search <text>` searches document identifiers, titles, headings, and body text in memory. Matching is case-insensitive and uses the whitespace-separated query terms as substrings. A title match has the highest score, followed by a heading match, an identifier match, and a body match. Carabiner prints up to 10 results in score order. Each result contains the document identifier, an em dash, and the first matching line. Context longer than 160 characters is shortened. Search does not use prefix or fuzzy expansion.

### Behavior

- `carabiner docs` without an argument lists every document identifier in sorted order, one per line.
- A missing document, invalid identifier, empty search text, search with no results, or document argument combined with `--search` exits with code `1` and an explanatory error.
- Document content is printed verbatim, so it can be piped to another tool.
- The global `--json` option is not supported because `docs` prints raw Markdown. It exits with code `1` when used.

## Release Notes Command

`release-notes` prints GitHub release notes for a repository. Use it to inspect changes in an upstream AI coding tool or in Carabiner without leaving the terminal. It retrieves the most recent 100 releases through the GitHub Releases API and renders them as Markdown in the returned order.

Supply a repository as `owner/repo` or a full GitHub URL. Unlike `fetch`, repository arguments cannot include a reference with `@` or a path suffix with `:`. Use `--tag` for one release. Only GitHub is supported because other Git providers do not expose the same Releases API.

### Usage

```bash
# The latest 10 releases
carabiner release-notes findyourexit/carabiner

# The five most recent releases
carabiner release-notes findyourexit/carabiner --latest 5

# Releases within a date range
carabiner release-notes findyourexit/carabiner --since 2026-01-01T00:00:00Z --until 2026-06-30

# One release by tag
carabiner release-notes findyourexit/carabiner --tag v0.1.0

# Every release between two tags, including both tags
carabiner release-notes findyourexit/carabiner --from v0.1.0 --to v0.2.0

# Include prereleases
carabiner release-notes findyourexit/carabiner --include-prereleases

# Structured output
carabiner --json release-notes findyourexit/carabiner --latest 3
```

### Filtering

`--latest`, `--since` and `--until`, `--tag`, and `--from` with `--to` are mutually exclusive. Combining filtering modes exits with code `1`. Without a filter, Carabiner prints the latest 10 releases.

| Option | Description |
| --- | --- |
| `--latest <count>` | Print the most recent count of releases. The count must be a positive integer. |
| `--since <date>` / `--until <date>` | Print releases published in the inclusive range. Either end can be omitted. `--since` accepts an RFC 3339 timestamp. A date supplied to `--until`, such as `2026-01-31`, includes that whole day in UTC. |
| `--tag <tag>` | Print one release by tag. The command uses `--tag` rather than `--version` because `--version` is the global flag that prints the Carabiner version. |
| `--from <tag>` / `--to <tag>` | Print every release between two tags, including both tags. Both options are required. |
| `--include-prereleases` | Include prereleases in list, date-range, and tag-range output. |
| `--token <token>` | GitHub token for private repositories or higher rate limits. |

Tag ranges use a tag's position in the retrieved release history rather than semantic version parsing. Non-semantic version tag names work, and the order of `--from` and `--to` does not matter. In a tag-range query, a missing tag produces an error when it is not among the 100 releases retrieved by the command.

### Authentication

Requests are unauthenticated by default. This is enough for public repositories but uses GitHub's stricter anonymous rate limit. Set `GITHUB_TOKEN` or `GH_TOKEN`, or pass `--token`, for private repositories and higher limits.

```bash
GITHUB_TOKEN=$(gh auth token) carabiner release-notes owner/private-repo
```

### Behavior

- Draft releases are never printed because they are unpublished and visible only to accounts with write access.
- Prereleases are excluded unless `--include-prereleases` is supplied. An explicitly named `--tag` is the exception and is printed even when it is a prerelease.
- A repository with no matching releases completes with code `0` and no release entries.
- A date-range query examines every release returned by the API rather than stopping at the first release outside the range. A release published from a long-lived branch can appear out of publication order.
- By default, output is Markdown on standard output and can be piped to another tool. With the global `--json` option, releases are emitted as structured `data` and no Markdown is printed. Failures use the standard error document with the `RELEASE_NOTES_FAILED` code.
