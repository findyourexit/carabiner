# Programmatic API

Carabiner exposes a Rust library for generating, importing, converting, and exporting AI-tool configuration. The crate root re-exports the primary operations, options, result types, `all_targets`, and `all_features`.

## Install the CLI

Install the CLI if you also use its command-line workflow:

```sh
cargo install carabiner --locked
```

## Add the crate to a Rust project

```toml
[dependencies]
anyhow = "1"
carabiner = "0.1.0"
```

The high-level operations return `anyhow::Result`. They resolve configuration through `Config::resolve`. Explicit option values take precedence over `carabiner.jsonc`, whose sibling `carabiner.local.jsonc` file overlays its values. Built-in defaults apply when neither an option nor a configuration file supplies a value.

## Generate configuration

`generate(options: GenerateOptions) -> anyhow::Result<GenerateResult>` reads a canonical source tree and writes target-specific configuration. `GenerateOptions` is the public type alias for `ConfigOptions` in `carabiner::engine`.

```rust
use anyhow::Result;
use carabiner::{generate, ConfigOptions};
use std::path::PathBuf;

fn main() -> Result<()> {
    let project = PathBuf::from("/path/to/project");
    let result = generate(ConfigOptions {
        cwd: Some(project),
        targets: Some(vec!["claudecode".into(), "cursor".into()]),
        features: Some(vec!["rules".into()]),
        ..ConfigOptions::default()
    })?;

    println!("Generated {} file(s)", result.total_files());
    for path in &result.rules.paths {
        println!("{path}");
    }
    Ok(())
}
```

Unless configured otherwise, the primary canonical source tree is `<cwd>/.carabiner/` and output is written below `<cwd>`. The source tree must contain the material required by each selected feature. For example, selecting `mcp` requires the canonical MCP source file.

### `ConfigOptions`

Every field is optional. `ConfigOptions::default()` leaves them unset so that configuration files and resolved defaults determine the behavior.

| Field | Type | When unset | Behavior |
| --- | --- | --- | --- |
| `cwd` | `Option<PathBuf>` | Current process directory | Base directory for relative configuration, input, and output paths. |
| `config_path` | `Option<String>` | `carabiner.jsonc` | Main configuration path. Carabiner also loads `carabiner.local.jsonc` from the same directory. |
| `targets` | `Option<Vec<String>>` | Configured targets or `["agentsmd"]` | Target identifiers from `all_targets()`. A `"*"` entry expands to the non-legacy targets. |
| `features` | `Option<Vec<String>>` | Configured features or `["rules"]` | Feature identifiers from `all_features()`. A `"*"` entry expands to every feature. |
| `output_roots` | `Option<Vec<String>>` | Configured roots or `[<cwd>]` | Destination roots for generated target files. |
| `input_roots` | `Option<Vec<String>>` | Configured roots or `[".carabiner"]` | Canonical source trees. The first root must be a directory. Later missing roots are allowed as optional overlays and later files replace earlier files with the same relative path. Cannot be combined with `input_root`. |
| `input_root` | `Option<String>` | `None` | Parent directory of a canonical source tree. Carabiner appends `.carabiner/`. Cannot be combined with `input_roots`. |
| `delete` | `Option<bool>` | Configured value or `false` | Removes target output that is no longer generated for enabled features. |
| `global` | `Option<bool>` | Configured value or `false` | Uses the target's user-scoped locations instead of project-scoped locations. A configured input root suppresses a `global` value from a configuration file. Set this option to `Some(true)` to select global output in that case. |
| `dry_run` | `Option<bool>` | Configured value or `false` | Plans changes without writing files. |
| `check` | `Option<bool>` | Configured value or `false` | Runs generation in preview mode and records differences in `GenerateResult::has_diff`. It cannot be combined with `dry_run`. Library calls return the result rather than assigning a CLI exit code. |
| `simulate_commands` | `Option<bool>` | Configured value or `false` | Allows simulated command output for targets without native command support. |
| `simulate_subagents` | `Option<bool>` | Configured value or `false` | Allows simulated subagent output for targets without native subagent support. |
| `simulate_skills` | `Option<bool>` | Configured value or `false` | Allows simulated skill output for targets without native skill support. |
| `flattened_command_naming` | `Option<String>` | Configured value or `"basename"` | Naming mode for targets that flatten commands. Accepted values are `"basename"` and `"path"`. |
| `verbose` and `silent` | `Option<bool>` | Configured value or `false` | CLI-compatible output settings. The high-level library calls return results and do not print command-line summaries. |
| `gitignore_targets_only` | `Option<bool>` | Configured value or `true` | Controls the CLI `gitignore` command's target selection when configuration exists. It does not control `generate`. |
| `gitignore_destination` | `Option<String>` | Configured value or `"gitignore"` | Controls whether the CLI `gitignore` command writes to `"gitignore"` or `"gitattributes"`. It does not control `generate`. |

`generate_flat(options)` accepts the same options and returns `FlatGenerateResult`.

### Targets and features

`all_targets()` returns the current 42 target identifiers. `all_features()` returns `rules`, `ignore`, `mcp`, `commands`, `subagents`, `skills`, `hooks`, `permissions`, and `checks`. Use these helpers when code needs the current supported values instead of maintaining its own list.

`Feature` provides the same values as an enum. Use `Feature::ALL` to iterate through them and `Feature::as_str()` when an API expects a feature string.

## Import configuration from a tool

`import_from_tool(options: ImportOptions) -> anyhow::Result<ImportResult>` reads one target's existing configuration and writes the canonical representation. For project scope, the canonical destination is under `<cwd>/.carabiner/`.

```rust
use anyhow::Result;
use carabiner::{import_from_tool, ImportOptions};
use std::path::PathBuf;

fn import_cursor_rules() -> Result<()> {
    let result = import_from_tool(ImportOptions {
        target: "cursor".into(),
        features: Some(vec!["rules".into()]),
        cwd: Some(PathBuf::from("/path/to/project")),
        ..ImportOptions::default()
    })?;

    println!("Imported {} file(s)", result.total_files());
    Ok(())
}
```

`target` is required and must be one identifier from `all_targets()`. `features`, `config_path`, `cwd`, `global`, `verbose`, and `silent` use the same configuration resolution rules as `ConfigOptions`. `output_root` is the root from which to read the target's existing configuration. When it is unset, Carabiner uses the resolved output root, normally `<cwd>` for project scope.

`ImportOptions::from_config(config)` constructs import options from `ConfigOptions`. `ImportOptions::to_config_options()` exposes the configuration form used during resolution. `import_from_tool_flat(options)` returns `FlatImportResult`.

## Convert directly between tools

`convert_from_tool(options: ConvertOptions) -> anyhow::Result<ImportResult>` reads one tool's configuration and renders it for one or more destination tools without creating intermediate `.carabiner/` files.

```rust
use anyhow::Result;
use carabiner::{convert_from_tool, ConvertOptions};
use std::path::PathBuf;

fn convert_claude_rules() -> Result<()> {
    let result = convert_from_tool(ConvertOptions {
        from: "claudecode".into(),
        to: vec!["cursor".into(), "copilot".into()],
        features: Some(vec!["rules".into()]),
        cwd: Some(PathBuf::from("/path/to/project")),
        ..ConvertOptions::default()
    })?;

    println!("Converted {} file(s)", result.total_files());
    Ok(())
}
```

`from` and `to` are required. Destination names are deduplicated. A destination cannot equal the source, and plugin packaging targets are not supported by conversion. `features`, `config_path`, `cwd`, `global`, `dry_run`, `verbose`, and `silent` are resolved through `ConfigOptions`. If `features` is unset, the library uses the configured feature set or the `rules` default.

`ConvertOptions::from_config(config, from, to)` constructs options from `ConfigOptions`. `ConvertOptions::to_config_options()` exposes the configuration form used during resolution. `ConvertResult` is a type alias for `ImportResult`, and `convert_from_tool_flat(options)` returns `FlatImportResult`.

## Export a canonical source tree

`export_canonical_to_tool_directory(target, source_root, output_root, features)` renders a canonical source tree into a target's native layout. Pass `source_root` directly as the canonical tree, such as `Path::new(".carabiner")`, and pass the destination directory as `output_root`.

```rust
use anyhow::Result;
use carabiner::export_canonical_to_tool_directory;
use std::path::Path;

fn export_for_cursor() -> Result<()> {
    let result = export_canonical_to_tool_directory(
        "cursor",
        Path::new(".carabiner"),
        Path::new("staging"),
        &["rules".into()],
    )?;

    println!("Exported {} file(s)", result.total_files());
    Ok(())
}
```

The function validates the target and feature strings and returns `GenerateResult`.

## Inspect input roots

`inspect_input_roots(input_roots: &[PathBuf]) -> InputRootInspection` checks configured canonical roots without generating files. Its `existing` and `missing` fields list the paths by directory status. Its optional `message` explains an invalid or missing primary root, or an overlay that exists but is not a directory.

## Results

`GenerateResult` contains one `FeatureResult` for each feature plus `activation`. Each `FeatureResult` has a `count` and the relative `paths` that changed. `GenerateResult::total_files()` sums those counts, `feature(Feature)` returns a feature result, and `flat()` returns `FlatGenerateResult`. `has_diff` is true when generation identifies changes.

`ImportResult` contains a count for each feature. `total_files()` sums them, and `flat()` returns `FlatImportResult`. Conversion returns this same result shape through the `ConvertResult` alias.

The flat result types are serializable. Their serialized count and path names use camelCase to match the command-line JSON contract.

## Errors and write behavior

All high-level operations return errors for invalid targets, invalid features, unreadable or malformed source files, and failed file operations. `generate` also errors when its primary input root is not a directory. Use `dry_run` with `ConfigOptions` or `ConvertOptions` to inspect planned changes without writing files. `check` is available only through `ConfigOptions` and reports differences through `GenerateResult::has_diff`.
