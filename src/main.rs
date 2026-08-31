use anyhow::{anyhow, Context, Result};
use carabiner::config::{Config, ConfigOptions};
use carabiner::engine::{
    convert_from_tool, export_canonical_to_tool_directory, generate, import_from_tool,
    import_tool_to_directory, ConvertOptions, ImportOptions,
};
use carabiner::model::{GenerateResult, ImportResult};
use carabiner::targets::{all_features, all_targets, target_spec, VERSION};
use carabiner::util::{
    assert_no_symlink_path, direct_dirs, home_dir, json_pretty, parse_frontmatter, parse_jsonc,
    read_jsonc, relative_slash, safe_name, safe_relative_path, stringify_frontmatter, walk_files,
    write_bytes, write_text, write_text_raw,
};
use clap::{ArgAction, Args, Parser, Subcommand};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
static WATCH_STOP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(unix)]
extern "C" fn watch_signal_handler(_: libc::c_int) {
    WATCH_STOP.store(true, std::sync::atomic::Ordering::SeqCst);
}

#[derive(Parser, Debug)]
#[command(
    name = "carabiner",
    version = VERSION,
    disable_version_flag = true,
    about = "Unified AI rules management CLI tool"
)]
struct Cli {
    #[arg(short = 'v', long = "version", action = ArgAction::Version)]
    _version: Option<bool>,
    #[arg(short = 'j', long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Init(CommonFlags),
    Generate(GenerateCli),
    Import(ImportCli),
    Convert(ConvertCli),
    Gitignore(GitignoreCli),
    Add(AddCli),
    Fetch(FetchCli),
    Install(InstallCli),
    Doctor(DoctorCli),
    Docs(DocsCli),
    #[command(name = "release-notes")]
    ReleaseNotes(ReleaseNotesCli),
    Update(UpdateCli),
    Mcp,
}

#[derive(Args, Debug, Clone, Default)]
struct CommonFlags {
    #[arg(short = 'V', long)]
    verbose: bool,
    #[arg(short = 's', long)]
    silent: bool,
}

#[derive(Args, Debug, Clone, Default)]
struct GenerateCli {
    #[arg(short = 't', long)]
    targets: Option<String>,
    #[arg(short = 'f', long)]
    features: Option<String>,
    #[arg(long)]
    delete: bool,
    #[arg(short = 'o', long = "output-roots")]
    output_roots: Option<String>,
    #[arg(short = 'c', long)]
    config: Option<String>,
    #[arg(short = 'g', long)]
    global: bool,
    #[arg(long)]
    simulate_commands: bool,
    #[arg(long)]
    simulate_subagents: bool,
    #[arg(long)]
    simulate_skills: bool,
    #[arg(long, num_args = 1..)]
    input_roots: Option<Vec<String>>,
    #[arg(long)]
    input_root: Option<String>,
    #[arg(long)]
    dry_run: bool,
    #[arg(long)]
    check: bool,
    #[arg(short = 'w', long)]
    watch: bool,
    #[arg(short = 'V', long)]
    verbose: bool,
    #[arg(short = 's', long)]
    silent: bool,
}

#[derive(Args, Debug, Clone, Default)]
struct ImportCli {
    #[arg(short = 't', long)]
    targets: Option<String>,
    #[arg(short = 'f', long)]
    features: Option<String>,
    #[arg(short = 'o', long = "output-root")]
    output_root: Option<String>,
    #[arg(short = 'c', long)]
    config: Option<String>,
    #[arg(short = 'g', long)]
    global: bool,
    #[arg(short = 'V', long)]
    verbose: bool,
    #[arg(short = 's', long)]
    silent: bool,
}

#[derive(Args, Debug, Clone, Default)]
struct ConvertCli {
    #[arg(long = "from")]
    from: String,
    #[arg(long = "to", required = true, value_delimiter = ',')]
    to: Vec<String>,
    #[arg(short = 'f', long)]
    features: Option<String>,
    #[arg(short = 'c', long)]
    config: Option<String>,
    #[arg(short = 'g', long)]
    global: bool,
    #[arg(long)]
    dry_run: bool,
    #[arg(short = 'V', long)]
    verbose: bool,
    #[arg(short = 's', long)]
    silent: bool,
}

#[derive(Args, Debug, Clone, Default)]
struct GitignoreCli {
    #[arg(short = 't', long)]
    targets: Option<String>,
    #[arg(short = 'f', long)]
    features: Option<String>,
    #[arg(short = 'V', long)]
    verbose: bool,
    #[arg(short = 's', long)]
    silent: bool,
}

#[derive(Args, Debug, Clone, Default)]
struct AddCli {
    source: String,
    #[arg(long)]
    name: Option<String>,
    #[arg(short = 'f', long)]
    force: bool,
    #[arg(long)]
    skills: Option<String>,
    #[arg(long)]
    rules: Option<String>,
    #[arg(long)]
    transport: Option<String>,
    #[arg(short = 'r', long = "ref")]
    reference: Option<String>,
    #[arg(short = 'p', long)]
    path: Option<String>,
    #[arg(long = "rules-path")]
    rules_path: Option<String>,
    #[arg(long)]
    registry: Option<String>,
    #[arg(long = "token-env")]
    token_env: Option<String>,
    #[arg(long)]
    token: Option<String>,
    #[arg(short = 'c', long)]
    config: Option<String>,
    #[arg(short = 'V', long)]
    verbose: bool,
    #[arg(short = 's', long)]
    silent: bool,
}

#[derive(Args, Debug, Clone, Default)]
struct FetchCli {
    source: String,
    #[arg(short = 't', long)]
    target: Option<String>,
    #[arg(short = 'f', long)]
    features: Option<String>,
    #[arg(short = 'r', long = "ref")]
    reference: Option<String>,
    #[arg(short = 'p', long)]
    path: Option<String>,
    #[arg(short = 'o', long)]
    output: Option<String>,
    #[arg(short = 'c', long, default_value = "overwrite")]
    conflict: String,
    #[arg(long)]
    skills: Option<String>,
    #[arg(short = 'i', long)]
    interactive: bool,
    #[arg(long)]
    token: Option<String>,
    #[arg(short = 'V', long)]
    verbose: bool,
    #[arg(short = 's', long)]
    silent: bool,
}

#[derive(Args, Debug, Clone, Default)]
struct InstallCli {
    #[arg(long)]
    mode: Option<String>,
    #[arg(long)]
    update: bool,
    #[arg(long)]
    frozen: bool,
    #[arg(long)]
    token: Option<String>,
    #[arg(short = 'c', long)]
    config: Option<String>,
    #[arg(short = 'V', long)]
    verbose: bool,
    #[arg(short = 's', long)]
    silent: bool,
}

#[derive(Args, Debug, Clone, Default)]
struct DoctorCli {
    #[arg(short = 'c', long)]
    config: Option<String>,
    #[arg(long)]
    strict: bool,
    #[arg(short = 'V', long)]
    verbose: bool,
    #[arg(short = 's', long)]
    silent: bool,
}

#[derive(Args, Debug, Clone, Default)]
struct DocsCli {
    document: Option<String>,
    #[arg(long)]
    search: Option<String>,
    #[arg(short = 'V', long)]
    verbose: bool,
    #[arg(short = 's', long)]
    silent: bool,
}

#[derive(Args, Debug, Clone, Default)]
struct ReleaseNotesCli {
    source: String,
    #[arg(long)]
    latest: Option<String>,
    #[arg(long)]
    since: Option<String>,
    #[arg(long)]
    until: Option<String>,
    #[arg(long)]
    tag: Option<String>,
    #[arg(long)]
    from: Option<String>,
    #[arg(long)]
    to: Option<String>,
    #[arg(long)]
    include_prereleases: bool,
    #[arg(long)]
    token: Option<String>,
    #[arg(short = 'V', long)]
    verbose: bool,
    #[arg(short = 's', long)]
    silent: bool,
}

#[derive(Args, Debug, Clone, Default)]
struct UpdateCli {
    #[arg(long)]
    check: bool,
    #[arg(long)]
    force: bool,
    #[arg(long)]
    repository: Option<String>,
    #[arg(long)]
    asset_prefix: Option<String>,
    #[arg(long)]
    token: Option<String>,
    #[arg(short = 'V', long)]
    verbose: bool,
    #[arg(short = 's', long)]
    silent: bool,
}

#[derive(Debug)]
struct Failure {
    code: &'static str,
    message: String,
    exit_code: i32,
    details: Option<Value>,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for Failure {}

fn main() {
    let cli = Cli::parse();
    let json_mode = cli.json;
    if json_mode {
        std::env::set_var("CARABINER_JSON", "1");
    }
    if json_mode && matches!(cli.command, Commands::Docs(_)) {
        print_json_error(
            "docs",
            "DOCS_FAILED",
            "The docs command prints raw Markdown and does not support --json.",
            None,
        );
        std::process::exit(1);
    }
    let command_name = command_name(&cli.command);
    match dispatch(cli.command) {
        Ok(data) => {
            if json_mode {
                print_json_success(command_name, data);
            }
        }
        Err(error) => {
            let failure = error.downcast_ref::<Failure>();
            let (code, message, exit_code, details) = failure
                .map(|failure| {
                    (
                        failure.code,
                        failure.message.clone(),
                        failure.exit_code,
                        failure.details.clone(),
                    )
                })
                .unwrap_or((
                    default_error_code(command_name),
                    format_error(&error),
                    1,
                    None,
                ));
            if json_mode {
                print_json_error(command_name, code, &message, details.as_ref());
            } else {
                eprintln!("{message}");
            }
            std::process::exit(exit_code);
        }
    }
}

fn command_name(command: &Commands) -> &'static str {
    match command {
        Commands::Init(_) => "init",
        Commands::Generate(_) => "generate",
        Commands::Import(_) => "import",
        Commands::Convert(_) => "convert",
        Commands::Gitignore(_) => "gitignore",
        Commands::Add(_) => "add",
        Commands::Fetch(_) => "fetch",
        Commands::Install(_) => "install",
        Commands::Doctor(_) => "doctor",
        Commands::Docs(_) => "docs",
        Commands::ReleaseNotes(_) => "release-notes",
        Commands::Update(_) => "update",
        Commands::Mcp => "mcp",
    }
}

fn default_error_code(command: &str) -> &'static str {
    match command {
        "init" => "INIT_FAILED",
        "generate" => "GENERATION_FAILED",
        "import" => "IMPORT_FAILED",
        "convert" => "CONVERT_FAILED",
        "gitignore" => "GITIGNORE_FAILED",
        "add" => "ADD_FAILED",
        "fetch" => "FETCH_FAILED",
        "install" => "INSTALL_FAILED",
        "doctor" => "DOCTOR_FAILED",
        "docs" => "DOCS_FAILED",
        "release-notes" => "RELEASE_NOTES_FAILED",
        "update" => "UPDATE_FAILED",
        "mcp" => "MCP_FAILED",
        _ => "UNKNOWN_ERROR",
    }
}

fn human_output() -> bool {
    std::env::var_os("CARABINER_JSON").is_none()
}

fn format_error(error: &anyhow::Error) -> String {
    error
        .chain()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(": ")
}

fn print_json_success(command: &str, data: Value) {
    println!("{}", serde_json::to_string_pretty(&json!({"success": true, "timestamp": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true), "command": command, "version": VERSION, "data": data})).expect("json! output is always serializable"));
}

fn print_json_error(command: &str, code: &str, message: &str, details: Option<&Value>) {
    let mut error = Map::from_iter([
        ("code".into(), Value::String(code.into())),
        ("message".into(), Value::String(message.into())),
    ]);
    if let Some(details) = details {
        error.insert("details".into(), details.clone());
    }
    eprintln!(
        "{}",
        serde_json::to_string_pretty(&json!({"success": false, "timestamp": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true), "command": command, "version": VERSION, "error": error})).expect("json! output is always serializable")
    );
}

fn dispatch(command: Commands) -> Result<Value> {
    match command {
        Commands::Init(flags) => init_command(&flags),
        Commands::Generate(options) => generate_command(&options),
        Commands::Import(options) => import_command(&options),
        Commands::Convert(options) => convert_command(&options),
        Commands::Gitignore(options) => gitignore_command(&options),
        Commands::Add(options) => add_command(&options),
        Commands::Fetch(options) => fetch_command(&options),
        Commands::Install(options) => install_command(&options),
        Commands::Doctor(options) => doctor_command(&options),
        Commands::Docs(options) => docs_command(&options),
        Commands::ReleaseNotes(options) => release_notes_command(&options),
        Commands::Update(options) => update_command(&options),
        Commands::Mcp => mcp_command(),
    }
}

fn parse_list(value: Option<&String>) -> Option<Vec<String>> {
    value.map(|value| {
        value
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .collect()
    })
}

fn generate_options(options: &GenerateCli) -> ConfigOptions {
    ConfigOptions {
        config_path: options.config.clone(),
        targets: parse_list(options.targets.as_ref()),
        features: parse_list(options.features.as_ref()),
        output_roots: parse_list(options.output_roots.as_ref()),
        delete: options.delete.then_some(true),
        global: options.global.then_some(true),
        verbose: options.verbose.then_some(true),
        silent: options.silent.then_some(true),
        simulate_commands: options.simulate_commands.then_some(true),
        simulate_subagents: options.simulate_subagents.then_some(true),
        simulate_skills: options.simulate_skills.then_some(true),
        dry_run: options.dry_run.then_some(true),
        check: options.check.then_some(true),
        input_root: options.input_root.clone(),
        input_roots: options.input_roots.clone(),
        ..ConfigOptions::default()
    }
}

fn init_command(flags: &CommonFlags) -> Result<Value> {
    let cwd = std::env::current_dir()?;
    let source = cwd.join(".carabiner");
    assert_no_symlink_path(&cwd, &source)?;
    fs::create_dir_all(&source)?;
    let samples = [
        (
            "rules/overview.md",
            r#"---
root: true
targets: ["*"]
description: "Project overview and general development guidelines"
globs: ["**/*"]
---

# Project Overview

## General Guidelines

- Use TypeScript for all new code
- Follow consistent naming conventions
- Write self-documenting code with clear variable and function names
- Prefer composition over inheritance
- Use meaningful comments for complex business logic

## Code Style

- Use 2 spaces for indentation
- Use semicolons
- Use double quotes for strings
- Use trailing commas in multi-line objects and arrays

## Architecture Principles

- Organize code by feature, not by file type
- Keep related files close together
- Use dependency injection for better testability
- Implement proper error handling
- Follow single responsibility principle
"#,
        ),
        (
            "mcp.jsonc",
            r#"{
  "$schema": "https://github.com/findyourexit/carabiner/releases/latest/download/mcp-schema.json",
  "mcpServers": {
    "deepwiki": {
      "type": "http",
      "url": "https://mcp.deepwiki.com/mcp",
      "env": {}
    },
    "carabiner": {
      "type": "stdio",
      "command": "carabiner",
      "args": ["mcp"],
      "env": {}
    },
    "playwright": {
      "type": "stdio",
      "command": "pnpm",
      "args": [
        "dlx",
        "@playwright/mcp",
        "--headless"
      ],
      "env": {}
    }
  }
}
"#,
        ),
        (
            "subagents/planner.md",
            r#"---
name: planner
targets: ["*"]
description: >-
  This is the general-purpose planner. The user asks the agent to plan to
  suggest a specification, implement a new feature, refactor the codebase, or
  fix a bug. This agent can be called by the user explicitly only.
claudecode:
  model: inherit
---

You are the planner for any tasks.

Based on the user's instruction, create a plan while analyzing the related files. Then, report the plan in detail. You can output files to @tmp/ if needed.

Attention, again, you are just the planner, so though you can read any files and run any commands for analysis, please don't write any code.
"#,
        ),
        (
            "skills/project-context/SKILL.md",
            r#"---
name: project-context
description: "Summarize the project context and key constraints"
targets: ["*"]
---

Summarize the project goals, core constraints, and relevant dependencies.
Call out any architecture decisions, shared conventions, and validation steps.
Keep the summary concise and ready to reuse in future tasks."#,
        ),
        (
            "hooks.jsonc",
            r#"{
  "version": 1,
  "hooks": {
    "postToolUse": [
      {
        "matcher": "Write|Edit",
        "command": ".carabiner/hooks/format.sh"
      }
    ]
  }
}
"#,
        ),
        (
            "permissions.jsonc",
            r#"{
  "$schema": "https://github.com/findyourexit/carabiner/releases/latest/download/permissions-schema.json",
  "permission": {
    "bash": {
      "git status": "allow",
      "git diff": "allow",
      "ls *": "allow",
      "rm -rf *": "deny",
      "*": "ask"
    },
    "edit": {
      "src/**": "allow"
    },
    "read": {
      ".env": "deny",
      "credentials/**": "deny"
    }
  },
  "codexcli": {
    "approval_policy": "on-request",
    "approvals_reviewer": "auto_review",
    "base_permission_profile": ":danger-full-access"
  }
}
"#,
        ),
    ];
    let mut created = Vec::new();
    let mut skipped = Vec::new();
    for (relative, content) in samples {
        let path = source.join(relative);
        if path.exists() {
            skipped.push(format!(".carabiner/{relative}"));
        } else {
            write_text_raw(&path, content, false)?;
            created.push(format!(".carabiner/{relative}"));
        }
    }
    let config_path = cwd.join("carabiner.jsonc");
    if config_path.exists() {
        skipped.push("carabiner.jsonc".into());
    } else {
        let config = json!({"$schema":"https://github.com/findyourexit/carabiner/releases/latest/download/config-schema.json","targets":["codexcli","claudecode","opencode"],"features":["rules","mcp","subagents","skills","hooks","permissions"],"outputRoots":["."],"delete":true,"verbose":false,"silent":false,"global":false,"simulateCommands":false,"simulateSubagents":false,"simulateSkills":false,"gitignoreTargetsOnly":true});
        write_text_raw(&config_path, &serde_json::to_string_pretty(&config)?, false)?;
        created.push("carabiner.jsonc".into());
    }
    if !flags.silent && human_output() {
        for path in &created {
            println!("Created {path}");
        }
        for path in &skipped {
            println!("Skipped {path} (already exists)");
        }
        println!("carabiner initialized successfully!");
        println!("Next steps:");
        println!("1. Edit .carabiner/**/*.md, .carabiner/skills/*/SKILL.md, .carabiner/mcp.jsonc, .carabiner/hooks.jsonc and .carabiner/permissions.jsonc");
        println!("2. Run 'carabiner generate' to create configuration files");
    }
    Ok(json!({"created": created, "skipped": skipped}))
}

fn generate_command(options: &GenerateCli) -> Result<Value> {
    if options.input_root.is_some() && options.input_roots.is_some() {
        return Err(anyhow!("--input-root and --input-roots cannot be combined"));
    }
    if options.watch && (options.check || options.dry_run || !human_output()) {
        return Err(anyhow!(
            "--watch cannot be combined with --check, --dry-run, --json."
        ));
    }
    let config_options = generate_options(options);
    let resolved_config = Config::resolve(&config_options)?;
    if options.watch && (resolved_config.check() || resolved_config.dry_run()) {
        return Err(anyhow!(
            "--watch cannot be combined with --check, --dry-run, --json."
        ));
    }
    if let Some(primary) = resolved_config.input_roots().first() {
        if !primary.is_dir() {
            return Err(Failure {
                code: "CARABINER_DIR_NOT_FOUND",
                message: format!(
                    "Carabiner source directory '{}' does not exist. Run 'carabiner init' first.",
                    primary.display()
                ),
                exit_code: 1,
                details: None,
            }
            .into());
        }
    }
    let result = generate(config_options.clone())?;
    let resolved_features = resolved_config.features();
    print_generate_result(
        &result,
        options.silent || resolved_config.silent(),
        &resolved_features,
        resolved_config.preview(),
        resolved_config.check(),
    );
    if options.watch {
        watch_loop(config_options)?;
    }
    if resolved_config.check() && result.has_diff {
        return Err(Failure {
            code: "GENERATION_FAILED",
            message: "Files are not up to date. Run 'carabiner generate' to update.".into(),
            exit_code: 1,
            details: None,
        }
        .into());
    }
    Ok(if human_output() {
        serde_json::to_value(result)?
    } else {
        generate_result_json(&result)
    })
}

fn watch_loop(options: ConfigOptions) -> Result<()> {
    let config = Config::resolve(&options)?;
    WATCH_STOP.store(false, std::sync::atomic::Ordering::SeqCst);
    #[cfg(unix)]
    unsafe {
        libc::signal(
            libc::SIGINT,
            watch_signal_handler as *const () as libc::sighandler_t,
        );
    }
    let mut previous = source_fingerprint(config.input_roots());
    if human_output() && !config.silent() {
        println!("Watching for changes");
        let _ = io::stdout().flush();
    }
    while !WATCH_STOP.load(std::sync::atomic::Ordering::SeqCst) {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if WATCH_STOP.load(std::sync::atomic::Ordering::SeqCst) {
            break;
        }
        let current = source_fingerprint(config.input_roots());
        if current == previous {
            continue;
        }
        previous = current;
        if human_output() && !config.silent() {
            println!("Change detected");
            let _ = io::stdout().flush();
        }
        match generate(options.clone()) {
            Ok(result) => {
                let features = config.features();
                print_generate_result(
                    &result,
                    config.silent(),
                    &features,
                    config.preview(),
                    config.check(),
                );
            }
            Err(error) => {
                if human_output() && !config.silent() {
                    eprintln!("Generation failed: {}", format_error(&error));
                }
            }
        }
    }
    if human_output() && !config.silent() {
        println!("Stopped watching.");
        let _ = io::stdout().flush();
    }
    Ok(())
}

fn source_fingerprint(roots: &[PathBuf]) -> u64 {
    let mut total = 0u64;
    for root in roots {
        for path in walk_files(root) {
            if let Ok(metadata) = fs::metadata(&path) {
                total = total.wrapping_add(metadata.len()).wrapping_add(
                    metadata
                        .modified()
                        .ok()
                        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                        .map(|duration| duration.as_nanos() as u64)
                        .unwrap_or(0),
                );
            }
        }
    }
    total
}

fn feature_label(count: usize, plural: &str) -> &'static str {
    if count != 1 {
        return match plural {
            "rules" => "rules",
            "ignore files" => "ignore files",
            "MCP files" => "MCP files",
            "commands" => "commands",
            "subagents" => "subagents",
            "skills" => "skills",
            "hooks" => "hooks",
            "permissions" => "permissions",
            "checks" => "checks",
            "Hermes activation files" => "Hermes activation files",
            _ => "items",
        };
    }
    match plural {
        "rules" => "rule",
        "ignore files" => "ignore file",
        "MCP files" => "MCP file",
        "commands" => "command",
        "subagents" => "subagent",
        "skills" => "skill",
        "hooks" => "hooks file",
        "permissions" => "permissions file",
        "checks" => "check",
        "Hermes activation files" => "Hermes activation file",
        _ => "items",
    }
}

fn print_generate_result(
    result: &GenerateResult,
    silent: bool,
    features: &[String],
    preview: bool,
    check: bool,
) {
    if silent || !human_output() {
        return;
    }
    for (label, feature) in [
        ("rules", &result.rules),
        ("ignore files", &result.ignore),
        ("MCP files", &result.mcp),
        ("commands", &result.commands),
        ("subagents", &result.subagents),
        ("skills", &result.skills),
        ("hooks", &result.hooks),
        ("permissions", &result.permissions),
        ("checks", &result.checks),
        ("Hermes activation files", &result.activation),
    ] {
        if feature.count > 0 {
            let verb = if preview { "Would write" } else { "Written" };
            let prefix = if preview { "[DRY RUN] " } else { "" };
            let label = feature_label(feature.count, label);
            println!("{prefix}{verb} {} {label}", feature.count);
            for path in &feature.paths {
                println!("    {path}");
            }
        }
    }
    if result.total_files() == 0 {
        if check {
            println!("✓ All files are up to date.");
        } else {
            println!("✓ All files are up to date ({})", features.join(", "));
        }
    } else {
        let parts = [
            (result.rules.count, "rules"),
            (result.ignore.count, "ignore files"),
            (result.mcp.count, "MCP files"),
            (result.commands.count, "commands"),
            (result.subagents.count, "subagents"),
            (result.skills.count, "skills"),
            (result.hooks.count, "hooks"),
            (result.permissions.count, "permissions"),
            (result.checks.count, "checks"),
            (result.activation.count, "Hermes activation files"),
        ]
        .into_iter()
        .filter(|(count, _)| *count > 0)
        .map(|(count, label)| format!("{count} {}", feature_label(count, label)))
        .collect::<Vec<_>>();
        if preview {
            println!(
                "[DRY RUN] Would write {} file(s) total ({})",
                result.total_files(),
                parts.join(" + ")
            );
        } else {
            println!(
                "🎉 All done! Written {} file(s) total ({})",
                result.total_files(),
                parts.join(" + ")
            );
        }
    }
}

fn feature_json(count: usize, paths: &[String]) -> Value {
    json!({"count": count, "paths": paths})
}

fn generate_result_json(result: &GenerateResult) -> Value {
    let features = json!({
        "ignore": feature_json(result.ignore.count, &result.ignore.paths),
        "mcp": feature_json(result.mcp.count, &result.mcp.paths),
        "commands": feature_json(result.commands.count, &result.commands.paths),
        "subagents": feature_json(result.subagents.count, &result.subagents.paths),
        "skills": feature_json(result.skills.count, &result.skills.paths),
        "hooks": feature_json(result.hooks.count, &result.hooks.paths),
        "permissions": feature_json(result.permissions.count, &result.permissions.paths),
        "checks": feature_json(result.checks.count, &result.checks.paths),
        "rules": feature_json(result.rules.count, &result.rules.paths),
        "activation": feature_json(result.activation.count, &result.activation.paths),
    });
    json!({
        "features": features,
        "totalFiles": result.total_files(),
        "hasDiff": result.has_diff,
        "skills": result.skill_details,
    })
}

fn feature_summary_parts(result: &ImportResult) -> Vec<String> {
    [
        (result.rules, "rules"),
        (result.ignore, "ignore files"),
        (result.mcp, "MCP files"),
        (result.commands, "commands"),
        (result.subagents, "subagents"),
        (result.skills, "skills"),
        (result.hooks, "hooks"),
        (result.permissions, "permissions"),
        (result.checks, "checks"),
    ]
    .into_iter()
    .filter(|(count, _)| *count > 0)
    .map(|(count, label)| format!("{count} {label}"))
    .collect()
}

fn import_result_json(result: &ImportResult, target: &str) -> Value {
    let features = json!({
        "rules": {"count": result.rules},
        "ignore": {"count": result.ignore},
        "mcp": {"count": result.mcp},
        "commands": {"count": result.commands},
        "subagents": {"count": result.subagents},
        "skills": {"count": result.skills},
        "hooks": {"count": result.hooks},
        "permissions": {"count": result.permissions},
        "checks": {"count": result.checks},
    });
    json!({"tool": target, "features": features, "totalFiles": result.total_files()})
}

fn convert_result_json(result: &ImportResult, from: &str, to: &[String], dry_run: bool) -> Value {
    let features = json!({
        "rules": {"count": result.rules},
        "ignore": {"count": result.ignore},
        "mcp": {"count": result.mcp},
        "commands": {"count": result.commands},
        "subagents": {"count": result.subagents},
        "skills": {"count": result.skills},
        "hooks": {"count": result.hooks},
        "permissions": {"count": result.permissions},
        "checks": {"count": result.checks},
    });
    json!({"from": from, "to": to, "dryRun": dry_run, "features": features, "totalFiles": result.total_files()})
}

fn import_command(options: &ImportCli) -> Result<Value> {
    let targets = parse_list(options.targets.as_ref()).ok_or_else(|| Failure {
        code: "IMPORT_FAILED",
        message: "No tools found in --targets".into(),
        exit_code: 1,
        details: None,
    })?;
    if targets.len() != 1 {
        return Err(Failure {
            code: "IMPORT_FAILED",
            message: "Only one tool can be imported at a time".into(),
            exit_code: 1,
            details: None,
        }
        .into());
    }
    let target = targets[0].clone();
    if target_spec(&target).is_none() {
        return Err(Failure {
            code: "IMPORT_FAILED",
            message: format!("Invalid tool target '{target}'."),
            exit_code: 1,
            details: None,
        }
        .into());
    }
    let result = import_from_tool(ImportOptions {
        target: target.clone(),
        features: parse_list(options.features.as_ref()),
        config_path: options.config.clone(),
        cwd: None,
        global: options.global.then_some(true),
        verbose: options.verbose.then_some(true),
        silent: options.silent.then_some(true),
        output_root: options.output_root.clone(),
    })?;
    let total = result.total_files();
    if total == 0 {
        if !options.silent && human_output() {
            println!(
                "No files imported for enabled features: {}",
                parse_list(options.features.as_ref())
                    .unwrap_or_else(all_features)
                    .join(", ")
            );
        }
        return Ok(json!({}));
    }
    if !options.silent && human_output() {
        let parts = feature_summary_parts(&result);
        println!("Imported {} file(s) total ({})", total, parts.join(" + "));
    }
    Ok(if human_output() {
        json!({})
    } else {
        import_result_json(&result, &target)
    })
}

fn convert_command(options: &ConvertCli) -> Result<Value> {
    let from = options.from.trim();
    if from.is_empty() || target_spec(from).is_none() {
        return Err(Failure {
            code: "CONVERT_FAILED",
            message: format!(
                "Invalid source tool '{from}'. Must be one of: {}",
                all_targets().join(", ")
            ),
            exit_code: 1,
            details: None,
        }
        .into());
    }
    if options.to.is_empty() {
        return Err(Failure {
            code: "CONVERT_FAILED",
            message: "--to is required and must not be empty".into(),
            exit_code: 1,
            details: None,
        }
        .into());
    }
    let mut to = Vec::new();
    for raw in &options.to {
        let target = raw.trim();
        if target.is_empty() || target_spec(target).is_none() {
            return Err(Failure {
                code: "CONVERT_FAILED",
                message: format!(
                    "Invalid destination tool '{target}'. Must be one of: {}",
                    all_targets().join(", ")
                ),
                exit_code: 1,
                details: None,
            }
            .into());
        }
        if !to.iter().any(|existing| existing == target) {
            to.push(target.to_owned());
        }
    }
    if to.iter().any(|target| target == from) {
        return Err(Failure {
            code: "CONVERT_FAILED",
            message: format!(
                "Destination tools must not include the source tool '{from}'. Converting a tool onto itself is likely a mistake and may cause lossy round-trips."
            ),
            exit_code: 1,
            details: None,
        }
        .into());
    }
    if std::iter::once(from)
        .chain(to.iter().map(String::as_str))
        .any(|target| matches!(target, "antigravity-plugin" | "claudecode-plugin"))
    {
        return Err(Failure {
            code: "CONVERT_FAILED",
            message: "Plugin packaging targets are not supported by convert. Use import --output-root and generate --output-roots with an explicit plugin directory.".into(),
            exit_code: 1,
            details: None,
        }
        .into());
    }
    let result = convert_from_tool(ConvertOptions {
        from: from.to_owned(),
        to: to.clone(),
        features: Some(parse_list(options.features.as_ref()).unwrap_or_else(all_features)),
        config_path: options.config.clone(),
        cwd: None,
        global: options.global.then_some(true),
        dry_run: options.dry_run.then_some(true),
        verbose: options.verbose.then_some(true),
        silent: options.silent.then_some(true),
    })?;
    let total = result.total_files();
    if total == 0 {
        if !options.silent && human_output() {
            println!(
                "No files converted for enabled features: {}",
                parse_list(options.features.as_ref())
                    .unwrap_or_else(all_features)
                    .join(", ")
            );
        }
        return Ok(json!({}));
    }
    let parts = feature_summary_parts(&result);
    if !options.silent && human_output() {
        let verb = if options.dry_run {
            "Would convert"
        } else {
            "Converted"
        };
        println!(
            "{} {} file(s) total from {} to {} ({})",
            verb,
            total,
            from,
            to.join(", "),
            parts.join(" + ")
        );
    }
    Ok(if human_output() {
        json!({})
    } else {
        convert_result_json(&result, from, &to, options.dry_run)
    })
}

fn title_from_name(name: &str) -> String {
    name.split(['-', '_', '.'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn scaffold(feature: &str, name: Option<&str>) -> Result<(String, String)> {
    let feature = match feature.to_ascii_lowercase().as_str() {
        "rule" | "rules" => "rule",
        "command" | "commands" => "command",
        "subagent" | "subagents" => "subagent",
        "skill" | "skills" => "skill",
        "check" | "checks" => "check",
        "mcp" => "mcp",
        "hooks" | "hook" => "hooks",
        "ignore" => "ignore",
        "permission" | "permissions" => "permissions",
        _ => return Err(anyhow!("unknown Carabiner feature '{feature}'")),
    };
    let normalized = if matches!(feature, "rule" | "command" | "subagent" | "skill" | "check") {
        Some(normalize_name(feature, name)?)
    } else {
        if name.is_some() {
            return Err(anyhow!("Feature '{feature}' does not accept --name"));
        }
        None
    };
    let relative = match feature {
        "rule" => format!(
            ".carabiner/rules/{}.md",
            normalized
                .as_deref()
                .expect("normalized is Some for named features")
        ),
        "command" => format!(
            ".carabiner/commands/{}.md",
            normalized
                .as_deref()
                .expect("normalized is Some for named features")
        ),
        "subagent" => format!(
            ".carabiner/subagents/{}.md",
            normalized
                .as_deref()
                .expect("normalized is Some for named features")
        ),
        "skill" => format!(
            ".carabiner/skills/{}/SKILL.md",
            normalized
                .as_deref()
                .expect("normalized is Some for named features")
        ),
        "check" => format!(
            ".carabiner/checks/{}.md",
            normalized
                .as_deref()
                .expect("normalized is Some for named features")
        ),
        "mcp" => ".carabiner/mcp.jsonc".into(),
        "hooks" => ".carabiner/hooks.jsonc".into(),
        "ignore" => ".carabiner/.aiignore".into(),
        "permissions" => ".carabiner/permissions.jsonc".into(),
        _ => unreachable!(),
    };
    let content = match feature {
        "rule" => {
            let name = normalized.as_deref().expect("normalized is Some for named features");
            if name == "overview" {
                r#"---
root: true
targets: ["*"]
description: "Project overview and general development guidelines"
globs: ["**/*"]
---

# Project Overview

## General Guidelines

- Use TypeScript for all new code
- Follow consistent naming conventions
- Write self-documenting code with clear variable and function names
- Prefer composition over inheritance
- Use meaningful comments for complex business logic

## Code Style

- Use 2 spaces for indentation
- Use semicolons
- Use double quotes for strings
- Use trailing commas in multi-line objects and arrays

## Architecture Principles

- Organize code by feature, not by file type
- Keep related files close together
- Use dependency injection for better testability
- Implement proper error handling
- Follow single responsibility principle
"#.into()
            } else {
                let title = title_from_name(name);
                format!("---\nroot: false\ntargets: [\"*\"]\ndescription: \"{title} guidelines\"\nglobs: [\"**/*\"]\n---\n\n# {title}\n\nDescribe the project guidance that should apply when the configured globs match.\n")
            }
        }
        "command" => {
            let name = normalized.as_deref().expect("normalized is Some for named features");
            if name == "review-pr" {
                r#"---
description: 'Review a pull request'
targets: ["*"]
---

target_pr = $ARGUMENTS

If target_pr is not provided, use the PR of the current branch.

Execute the following in parallel:

1. Check code quality and style consistency
2. Review test coverage
3. Verify documentation updates
4. Check for potential bugs or security issues

Then provide a summary of findings and suggestions for improvement.
"#.into()
            } else {
                let title = title_from_name(name);
                format!("---\ndescription: \"Run the {title} workflow\"\ntargets: [\"*\"]\n---\n\n# {title}\n\nUse $ARGUMENTS as input and describe the steps this command should perform.\n")
            }
        }
        "subagent" => {
            let name = normalized.as_deref().expect("normalized is Some for named features");
            if name == "planner" {
                r#"---
name: planner
targets: ["*"]
description: >-
  This is the general-purpose planner. The user asks the agent to plan to
  suggest a specification, implement a new feature, refactor the codebase, or
  fix a bug. This agent can be called by the user explicitly only.
claudecode:
  model: inherit
---

You are the planner for any tasks.

Based on the user's instruction, create a plan while analyzing the related files. Then, report the plan in detail. You can output files to @tmp/ if needed.

Attention, again, you are just the planner, so though you can read any files and run any commands for analysis, please don't write any code.
"#.into()
            } else {
                let title = title_from_name(name);
                format!("---\nname: {}\ntargets: [\"*\"]\ndescription: \"{title} specialist\"\n---\n\nYou are the {title} specialist. Describe the role, constraints, and expected output here.\n", serde_json::to_string(name)?)
            }
        }
        "skill" => {
            let name = normalized.as_deref().expect("normalized is Some for named features");
            if name == "project-context" {
                r#"---
name: project-context
description: "Summarize the project context and key constraints"
targets: ["*"]
---

Summarize the project goals, core constraints, and relevant dependencies.
Call out any architecture decisions, shared conventions, and validation steps.
Keep the summary concise and ready to reuse in future tasks."#.into()
            } else {
                let title = title_from_name(name);
                format!("---\nname: {}\ndescription: \"Use {title} guidance for relevant tasks\"\ntargets: [\"*\"]\n---\n\n# {title}\n\nDescribe when to use this skill and the workflow it should follow.\n", serde_json::to_string(name)?)
            }
        }
        "check" => {
            let title = title_from_name(normalized.as_deref().expect("normalized is Some for named features"));
            format!("---\ntargets: [\"*\"]\ndescription: \"{title} review criteria\"\nseverity: medium\n---\n\n# {title}\n\nDescribe the conditions this check should detect and the evidence it should report.\n")
        }
        "mcp" => r#"{
  "$schema": "https://github.com/findyourexit/carabiner/releases/latest/download/mcp-schema.json",
  "mcpServers": {
    "deepwiki": {
      "type": "http",
      "url": "https://mcp.deepwiki.com/mcp",
      "env": {}
    },
    "carabiner": {
      "type": "stdio",
      "command": "carabiner",
      "args": ["mcp"],
      "env": {}
    },
    "playwright": {
      "type": "stdio",
      "command": "pnpm",
      "args": ["dlx", "@playwright/mcp", "--headless"],
      "env": {}
    }
  }
}
"#.into(),
        "hooks" => r#"{
  "version": 1,
  "hooks": {
    "postToolUse": [
      {
        "matcher": "Write|Edit",
        "command": ".carabiner/hooks/format.sh"
      }
    ]
  }
}
"#.into(),
        "ignore" => "credentials/\n".into(),
        "permissions" => r#"{
  "$schema": "https://github.com/findyourexit/carabiner/releases/latest/download/permissions-schema.json",
  "permission": {
    "bash": {
      "git status": "allow",
      "git diff": "allow",
      "ls *": "allow",
      "rm -rf *": "deny",
      "*": "ask"
    },
    "edit": {
      "src/**": "allow"
    },
    "read": {
      ".env": "deny",
      "credentials/**": "deny"
    }
  },
  "codexcli": {
    "approval_policy": "on-request",
    "approvals_reviewer": "auto_review",
    "base_permission_profile": ":danger-full-access"
  }
}
"#.into(),
        _ => unreachable!(),
    };
    Ok((relative, content))
}

fn normalize_name(feature: &str, name: Option<&str>) -> Result<String> {
    let raw = name
        .ok_or_else(|| anyhow!("Feature '{feature}' requires --name <name>"))?
        .trim();
    let value = if raw.to_ascii_lowercase().ends_with(".md") {
        raw[..raw.len() - 3].to_owned()
    } else {
        raw.to_owned()
    };
    if value.is_empty()
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
        })
        || !value.chars().next().unwrap_or(' ').is_ascii_alphanumeric()
    {
        return Err(anyhow!("Invalid {feature} name '{value}'"));
    }
    Ok(value)
}

fn jsonc_matching_delimiter(content: &str, start: usize, open: u8, close: u8) -> Option<usize> {
    let bytes = content.as_bytes();
    let mut depth = 0usize;
    let mut index = start;
    let mut in_string = false;
    let mut escaped = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        if byte == b'"' {
            in_string = true;
            index += 1;
            continue;
        }
        if byte == b'/' && bytes.get(index + 1) == Some(&b'/') {
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            continue;
        }
        if byte == b'/' && bytes.get(index + 1) == Some(&b'*') {
            index += 2;
            while index + 1 < bytes.len() && !(bytes[index] == b'*' && bytes[index + 1] == b'/') {
                index += 1;
            }
            index = (index + 2).min(bytes.len());
            continue;
        }
        if byte == open {
            depth += 1;
        } else if byte == close {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(index);
            }
        }
        index += 1;
    }
    None
}

fn append_source_entry_jsonc(content: &str, entry: &Value) -> Result<String> {
    let entry = serde_json::to_string(entry)?;
    if let Some(key_start) = content.find("\"sources\"") {
        let open = content[key_start..]
            .find('[')
            .map(|offset| key_start + offset)
            .ok_or_else(|| anyhow!("'sources' must be an array"))?;
        let close = jsonc_matching_delimiter(content, open, b'[', b']')
            .ok_or_else(|| anyhow!("'sources' array is not closed"))?;
        let before_close = &content[..close];
        let insertion = before_close.trim_end_matches(char::is_whitespace).len();
        let has_entries = !content[open + 1..insertion].trim().is_empty();
        let insertion_text = if has_entries {
            format!(",\n    {entry}")
        } else {
            format!("\n    {entry}\n  ")
        };
        let mut updated = String::with_capacity(content.len() + insertion_text.len());
        updated.push_str(&content[..insertion]);
        updated.push_str(&insertion_text);
        updated.push_str(&content[insertion..]);
        if !updated.ends_with('\n') {
            updated.push('\n');
        }
        return Ok(updated);
    }
    let open = content
        .find('{')
        .ok_or_else(|| anyhow!("configuration must contain an object"))?;
    let close = jsonc_matching_delimiter(content, open, b'{', b'}')
        .ok_or_else(|| anyhow!("configuration object is not closed"))?;
    let before_close = &content[..close];
    let insertion = before_close.trim_end_matches(char::is_whitespace).len();
    let has_members = !content[open + 1..insertion].trim().is_empty();
    let insertion_text = if has_members {
        format!(",\n  \"sources\": [\n    {entry}\n  ]\n")
    } else {
        format!("\n  \"sources\": [\n    {entry}\n  ]\n")
    };
    let mut updated = String::with_capacity(content.len() + insertion_text.len());
    updated.push_str(&content[..insertion]);
    updated.push_str(&insertion_text);
    updated.push_str(&content[insertion..]);
    if !updated.ends_with('\n') {
        updated.push('\n');
    }
    Ok(updated)
}

fn validate_add_source(options: &AddCli) -> Result<()> {
    if let Some(transport) = options.transport.as_deref() {
        if !matches!(transport, "github" | "git" | "npm") {
            return Err(anyhow!(
                "Invalid source transport '{transport}'. Expected one of: github, git, npm."
            ));
        }
    }
    if (options.registry.is_some() || options.token_env.is_some())
        && options.transport.as_deref() != Some("npm")
    {
        return Err(anyhow!(
            "'registry' and 'tokenEnv' are only valid with transport 'npm'."
        ));
    }
    if let Some(registry) = options.registry.as_deref() {
        if !registry.starts_with("https://") && !registry.starts_with("http://") {
            return Err(anyhow!("registry must be an http(s) URL"));
        }
    }
    if let Some(token_env) = options.token_env.as_deref() {
        if token_env.is_empty()
            || !token_env.chars().enumerate().all(|(index, character)| {
                character == '_'
                    || character.is_ascii_alphanumeric()
                        && (index > 0 || character.is_ascii_alphabetic())
            })
        {
            return Err(anyhow!(
                "tokenEnv must be a valid environment variable name"
            ));
        }
    }
    if options.source.starts_with("http://") || options.source.starts_with("https://") {
        if let Some(authority) = options
            .source
            .split_once("://")
            .and_then(|(_, rest)| rest.split('/').next())
        {
            if authority.contains('@') {
                return Err(anyhow!("Source URLs must not contain credentials. Use an environment variable, credential helper, or SSH authentication instead."));
            }
        }
    }
    for (name, value) in [
        ("path", options.path.as_deref()),
        ("rulesPath", options.rules_path.as_deref()),
    ] {
        if value.is_some_and(|value| {
            Path::new(value).is_absolute() || value.split(&['/', '\\'][..]).any(|part| part == "..")
        }) {
            return Err(anyhow!(
                "{name} must be a relative path without '..' segments"
            ));
        }
    }
    Ok(())
}

fn add_command(options: &AddCli) -> Result<Value> {
    let feature_names = [
        "rule",
        "rules",
        "command",
        "commands",
        "subagent",
        "subagents",
        "skill",
        "skills",
        "check",
        "checks",
        "mcp",
        "hooks",
        "hook",
        "ignore",
        "permission",
        "permissions",
    ];
    let is_feature = feature_names
        .iter()
        .any(|feature| feature == &options.source.to_ascii_lowercase());
    let source_only_options = options.skills.is_some()
        || options.rules.is_some()
        || options.transport.is_some()
        || options.reference.is_some()
        || options.path.is_some()
        || options.rules_path.is_some()
        || options.registry.is_some()
        || options.token_env.is_some()
        || options.token.is_some()
        || options.config.is_some();
    if is_feature {
        if source_only_options {
            return Err(anyhow!(
                "Feature scaffold options cannot be combined with declarative source options"
            ));
        }
        let (canonical_relative, content) = scaffold(&options.source, options.name.as_deref())?;
        let feature_name = options.source.to_ascii_lowercase();
        let mut candidates = vec![canonical_relative.clone()];
        match feature_name.as_str() {
            "mcp" => {
                candidates.extend([".carabiner/mcp.json".into(), ".carabiner/.mcp.json".into()])
            }
            "hooks" | "hook" => candidates.push(".carabiner/hooks.json".into()),
            "permissions" | "permission" => candidates.push(".carabiner/permissions.json".into()),
            "ignore" => candidates.push(".carabinerignore".into()),
            _ => {}
        }
        let relative = candidates
            .iter()
            .find(|candidate| Path::new(candidate).is_file())
            .cloned()
            .unwrap_or(canonical_relative);
        let path = PathBuf::from(&relative);
        let cwd = std::env::current_dir()?.canonicalize()?;
        assert_no_symlink_path(&cwd, &cwd.join(&path))?;
        if let Some(parent) = path.parent().filter(|parent| parent.exists()) {
            if !fs::canonicalize(parent)?.starts_with(&cwd) {
                return Err(anyhow!(
                    "Refusing to write outside the project root through a symbolic link"
                ));
            }
        }
        if fs::symlink_metadata(&path)
            .map(|metadata| metadata.file_type().is_symlink())
            .unwrap_or(false)
        {
            return Err(anyhow!(
                "Refusing to write through a symbolic link: {relative}"
            ));
        }
        if path.exists() && !options.force {
            if options.silent || !human_output() {
                return Err(anyhow!("Refusing to prompt before overwriting {relative} in JSON or silent mode. Re-run with --force to replace it."));
            }
            if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
                return Err(anyhow!("Refusing to overwrite {relative} in non-interactive mode. Re-run with --force to replace it."));
            }
            print!("Overwrite {relative}? [y/N] ");
            io::stdout().flush()?;
            let mut answer = String::new();
            io::stdin().read_line(&mut answer)?;
            if !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
                if human_output() {
                    println!("Kept {relative} unchanged.");
                }
                return Ok(json!({"created": [], "skipped": [relative]}));
            }
        }
        write_text_raw(&path, &content, false)?;
        if !options.silent && human_output() {
            println!("Created {relative}");
        }
        return Ok(json!({"created": [relative], "skipped": []}));
    }
    if options.name.is_some() || options.force {
        return Err(anyhow!(
            "--name and --force are only valid when adding a Carabiner feature file"
        ));
    }
    validate_add_source(options)?;
    let project_root = std::env::current_dir()?.canonicalize()?;
    let relative_config = options
        .config
        .clone()
        .unwrap_or_else(|| "carabiner.jsonc".into());
    let raw_config_path = PathBuf::from(&relative_config);
    let config_path = if raw_config_path.is_absolute() {
        raw_config_path
    } else {
        project_root.join(raw_config_path)
    };
    if !config_path.is_file() {
        return Err(anyhow!(
            "Configuration file not found: {relative_config}. Run 'carabiner init' first or pass --config."
        ));
    }
    let config_real = fs::canonicalize(&config_path)?;
    if !config_real.starts_with(&project_root) {
        return Err(anyhow!(
            "Configuration file must resolve inside the project root: {relative_config}."
        ));
    }
    if fs::symlink_metadata(&config_path)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err(anyhow!(
            "Refusing to write through a symbolic link: {relative_config}"
        ));
    }
    let original = fs::read_to_string(&config_path)?;
    let root =
        parse_jsonc(&original).with_context(|| format!("Failed to parse {relative_config}"))?;
    let object = root
        .as_object()
        .ok_or_else(|| anyhow!("configuration must contain an object"))?;
    let transport = options.transport.as_deref().unwrap_or("github");
    let identity = format!("{}:{}", transport, normalized_source_key(&options.source));
    if let Some(existing) = object.get("sources").and_then(Value::as_array) {
        if existing.iter().any(|value| {
            let Some(value) = value.as_object() else {
                return false;
            };
            let existing_transport = value
                .get("transport")
                .and_then(Value::as_str)
                .unwrap_or("github");
            value
                .get("source")
                .and_then(Value::as_str)
                .map(|source| {
                    format!("{}:{}", existing_transport, normalized_source_key(source)) == identity
                })
                .unwrap_or(false)
        }) {
            return Err(anyhow!(
                "Source \"{}\" is already declared in {relative_config}. Edit the existing entry to change its options.",
                options.source
            ));
        }
    }
    let backup_root = std::env::temp_dir().join(format!(
        "carabiner-add-backup-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let _backup_guard = TempDirGuard::new(Some(backup_root.clone()));
    fs::create_dir_all(&backup_root)?;
    let curated_skills = project_root.join(".carabiner/skills/.curated");
    let curated_rules = project_root.join(".carabiner/rules/.curated");
    let sources_lock = project_root.join("carabiner.lock");
    let npm_sources_lock = project_root.join("carabiner-npm.lock.json");
    let curated_skills_existed = curated_skills.is_dir();
    let curated_rules_existed = curated_rules.is_dir();
    if curated_skills_existed {
        copy_tree(
            &curated_skills,
            &backup_root.join("curated-skills"),
            "overwrite",
        )?;
    }
    if curated_rules_existed {
        copy_tree(
            &curated_rules,
            &backup_root.join("curated-rules"),
            "overwrite",
        )?;
    }
    let sources_lock_content = sources_lock
        .is_file()
        .then(|| fs::read(&sources_lock))
        .transpose()?;
    let npm_sources_lock_content = npm_sources_lock
        .is_file()
        .then(|| fs::read(&npm_sources_lock))
        .transpose()?;

    let mut source_entry = Map::new();
    source_entry.insert("source".into(), Value::String(options.source.clone()));
    if let Some(value) = parse_list(options.skills.as_ref()) {
        source_entry.insert(
            "skills".into(),
            Value::Array(value.into_iter().map(Value::String).collect()),
        );
    }
    if let Some(value) = parse_list(options.rules.as_ref()) {
        source_entry.insert(
            "rules".into(),
            Value::Array(value.into_iter().map(Value::String).collect()),
        );
    }
    for (key, value) in [
        ("transport", &options.transport),
        ("ref", &options.reference),
        ("path", &options.path),
        ("rulesPath", &options.rules_path),
        ("registry", &options.registry),
        ("tokenEnv", &options.token_env),
    ] {
        if let Some(value) = value {
            source_entry.insert(key.into(), Value::String(value.clone()));
        }
    }
    let updated = append_source_entry_jsonc(&original, &Value::Object(source_entry))?;
    let parsed_updated = parse_jsonc(&updated)
        .with_context(|| format!("Failed to validate updated {relative_config}"))?;
    if !parsed_updated.is_object() {
        return Err(anyhow!("configuration must contain an object"));
    }
    fs::write(&config_path, updated.as_bytes())?;
    let install_options = InstallCli {
        update: true,
        token: options.token.clone(),
        config: options.config.clone(),
        verbose: options.verbose,
        silent: options.silent,
        ..InstallCli::default()
    };
    let restore = || -> Result<()> {
        fs::write(&config_path, original.as_bytes())?;
        for path in [&curated_skills, &curated_rules] {
            let _ = fs::remove_dir_all(path);
            let _ = fs::remove_file(path);
        }
        if curated_skills_existed {
            copy_tree(
                &backup_root.join("curated-skills"),
                &curated_skills,
                "overwrite",
            )?;
        }
        if curated_rules_existed {
            copy_tree(
                &backup_root.join("curated-rules"),
                &curated_rules,
                "overwrite",
            )?;
        }
        for (path, content) in [
            (&sources_lock, &sources_lock_content),
            (&npm_sources_lock, &npm_sources_lock_content),
        ] {
            match content {
                Some(content) => {
                    if let Some(parent) = path.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    fs::write(path, content)?;
                }
                None => {
                    let _ = fs::remove_file(path);
                }
            }
        }
        Ok(())
    };

    let install_result = match install_carabiner_sources(&install_options) {
        Ok(result) => result,
        Err(error) => {
            if let Err(restore_error) = restore() {
                return Err(anyhow!(
                    "Failed to install source \"{}\" and restore {}: {}; restore failed: {}",
                    options.source,
                    relative_config,
                    error,
                    restore_error
                ));
            }
            return Err(anyhow!(
                "Failed to install source \"{}\"; restored {}: {}",
                options.source,
                relative_config,
                error
            ));
        }
    };
    let mut data = install_result.as_object().cloned().unwrap_or_default();
    data.insert("source".into(), Value::String(options.source.clone()));
    data.insert("configPath".into(), Value::String(relative_config.clone()));
    if !options.silent && human_output() {
        println!(
            "Added \"{}\" to {} and installed {} skill(s) and {} rule(s).",
            options.source,
            relative_config,
            data.get("skillsFetched")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            data.get("rulesFetched")
                .and_then(Value::as_u64)
                .unwrap_or(0)
        );
    }
    Ok(Value::Object(data))
}

fn gitignore_command(options: &GitignoreCli) -> Result<Value> {
    let config_options = ConfigOptions {
        verbose: options.verbose.then_some(true),
        silent: options.silent.then_some(true),
        ..ConfigOptions::default()
    };
    let config = Config::resolve(&config_options)?;
    let explicit_targets = parse_list(options.targets.as_ref());
    let mut targets = if let Some(targets) = explicit_targets {
        targets
    } else if config.config_exists() && config.gitignore_targets_only() {
        config.targets().to_vec()
    } else {
        all_targets()
    };
    if targets.iter().any(|target| target == "*") {
        targets = all_targets();
    }
    if options.targets.is_none()
        && config.config_exists()
        && !targets.iter().any(|target| target == "agentsmd")
    {
        targets.push("agentsmd".into());
    }
    let mut features = parse_list(options.features.as_ref()).unwrap_or_else(all_features);
    if features.iter().any(|feature| feature == "*") {
        features = all_features();
    }
    for feature in &features {
        if !all_features().iter().any(|known| known == feature) {
            return Err(anyhow!("Invalid feature '{feature}'."));
        }
    }
    let mut gitignore = Vec::new();
    let mut gitattributes = Vec::new();
    let mut derived_gitignore: Vec<(usize, usize, usize, String)> = Vec::new();
    let mut derived_gitattributes: Vec<(usize, usize, usize, String)> = Vec::new();
    let selected_targets = targets.clone();
    for (entry_targets, feature, entry) in hand_gitignore_entries() {
        if feature != "general" && !features.iter().any(|value| value == feature) {
            continue;
        }
        if entry_targets.contains(&"common") {
            gitignore.push(entry.into());
            continue;
        }
        for target in &selected_targets {
            if !entry_targets.iter().any(|candidate| candidate == target) {
                continue;
            }
            if matches!(
                target.as_str(),
                "agentsskills"
                    | "augmentcode-legacy"
                    | "claudecode-legacy"
                    | "antigravity-plugin"
                    | "claudecode-plugin"
            ) {
                continue;
            }
            let feature_name = (feature != "general").then_some(feature);
            if config.gitignore_destination(target, feature_name) == "gitattributes" {
                gitattributes.push(entry.into());
            } else {
                gitignore.push(entry.into());
            }
        }
    }
    let feature_order = |feature: Option<&str>| match feature {
        Some("rules") => 0,
        Some("commands") => 1,
        Some("skills") => 2,
        Some("subagents") => 3,
        Some("mcp") => 4,
        Some("hooks") => 5,
        Some("permissions") => 6,
        Some("checks") => 7,
        Some("ignore") => 8,
        _ => 9,
    };
    for target in targets {
        if matches!(
            target.as_str(),
            "agentsskills"
                | "augmentcode-legacy"
                | "claudecode-legacy"
                | "antigravity-plugin"
                | "claudecode-plugin"
        ) {
            continue;
        }
        let spec = target_spec(&target).ok_or_else(|| anyhow!("Invalid tool target '{target}'"))?;
        let paths = spec.paths(false);
        let mut entries: Vec<(String, Option<String>)> = Vec::new();
        let add =
            |entries: &mut Vec<(String, Option<String>)>, value: String, feature: Option<&str>| {
                if !is_shared_output(&value) {
                    entries.push((format!("**/{value}"), feature.map(ToOwned::to_owned)));
                }
            };
        if features.iter().any(|feature| feature == "rules") {
            if let Some(root) = &paths.root_rule {
                add(&mut entries, root.path(), Some("rules"));
            }
            if target == "claudecode" {
                add(&mut entries, ".claude/CLAUDE.md".into(), Some("rules"));
            }
            if let Some(dir) = &paths.nonroot_rule_dir {
                add(&mut entries, format!("{dir}/"), Some("rules"));
            }
        }
        if features.iter().any(|feature| feature == "rules") && target == "pi" {
            add(&mut entries, ".pi/APPEND_SYSTEM.md".into(), Some("rules"));
            add(&mut entries, "AGENTS.override.md".into(), Some("rules"));
        }
        if features.iter().any(|feature| feature == "commands") {
            if let Some(dir) = &paths.command_dir {
                add(&mut entries, format!("{dir}/"), Some("commands"));
            }
        }
        if features.iter().any(|feature| feature == "subagents") {
            if let Some(dir) = &paths.subagent_dir {
                add(
                    &mut entries,
                    if paths.aggregate_subagents {
                        ".roomodes".into()
                    } else {
                        format!("{dir}/")
                    },
                    Some("subagents"),
                );
            }
        }
        if features.iter().any(|feature| feature == "skills") {
            if let Some(dir) = &paths.skill_dir {
                add(&mut entries, format!("{dir}/"), Some("skills"));
            }
        }
        if features.iter().any(|feature| feature == "mcp") {
            if let Some(path) = &paths.mcp {
                add(&mut entries, path.path(), Some("mcp"));
            }
        }
        if features.iter().any(|feature| feature == "hooks") {
            if let Some(path) = &paths.hooks {
                add(&mut entries, path.path(), Some("hooks"));
            }
        }
        if features.iter().any(|feature| feature == "permissions") {
            if let Some(path) = &paths.permissions {
                add(&mut entries, path.path(), Some("permissions"));
            }
        }
        if features.iter().any(|feature| feature == "checks")
            && !matches!(target.as_str(), "augmentcode" | "cursor" | "rovodev")
        {
            if let Some(path) = &paths.checks {
                add(
                    &mut entries,
                    if path.file == "__dynamic__" {
                        format!("{}/", path.dir)
                    } else {
                        path.path()
                    },
                    Some("checks"),
                );
            }
        }
        if features.iter().any(|feature| feature == "ignore") {
            if let Some(path) = &paths.ignore {
                add(&mut entries, path.path(), Some("ignore"));
            }
        }
        let target_order = all_targets()
            .iter()
            .position(|known| *known == target)
            .unwrap_or(usize::MAX);
        for (entry_order, (entry, feature)) in entries.into_iter().enumerate() {
            let ordered = (
                feature_order(feature.as_deref()),
                target_order,
                entry_order,
                entry,
            );
            if config.gitignore_destination(&target, feature.as_deref()) == "gitattributes" {
                derived_gitattributes.push(ordered);
            } else {
                derived_gitignore.push(ordered);
            }
        }
    }
    derived_gitignore.sort();
    gitignore.extend(derived_gitignore.into_iter().map(|(_, _, _, entry)| entry));
    derived_gitattributes.sort();
    gitattributes.extend(
        derived_gitattributes
            .into_iter()
            .map(|(_, _, _, entry)| entry),
    );
    let mut seen_gitignore = HashSet::new();
    gitignore
        .retain(|entry| entry != "!.carabiner/.aiignore" && seen_gitignore.insert(entry.clone()));
    gitignore.push("!.carabiner/.aiignore".into());
    let mut seen_gitattributes = HashSet::new();
    gitattributes.retain(|entry| seen_gitattributes.insert(entry.clone()));
    let mut known_entries = gitignore.clone();
    known_entries.extend(gitattributes.iter().cloned());
    let added_gitignore = update_ignore_file(Path::new(".gitignore"), &gitignore, &known_entries)?;
    let added_attributes =
        update_ignore_file(Path::new(".gitattributes"), &gitattributes, &known_entries)?;
    if !options.silent && human_output() {
        println!("Updated .gitignore and .gitattributes");
    }
    Ok(
        json!({"gitignore": gitignore, "gitattributes": gitattributes, "updated": added_gitignore || added_attributes}),
    )
}

fn is_shared_output(path: &str) -> bool {
    matches!(
        path,
        ".amp/settings.json"
            | ".amp/settings.jsonc"
            | ".antigravity/settings.json"
            | ".claude/settings.json"
            | ".claude/settings.local.json"
            | ".codex/config.toml"
            | ".copilot/settings.json"
            | ".github/copilot/settings.json"
            | ".devin/config.json"
            | ".factory/settings.json"
            | ".grok/config.toml"
            | ".rovodev/config.yml"
            | ".rovodev/mcp.json"
            | ".vibe/config.toml"
            | "reasonix.toml"
            | ".vscode/settings.json"
            | ".zed/settings.json"
            | "kilo.json"
            | "kilo.jsonc"
            | "opencode.json"
            | "opencode.jsonc"
    )
}

fn hand_gitignore_entries() -> Vec<(Vec<&'static str>, &'static str, &'static str)> {
    vec![
        (vec!["common"], "general", ".carabiner/skills/.curated/"),
        (vec!["common"], "general", ".carabiner/rules/.curated/"),
        (vec!["common"], "general", ".carabiner/rules/*.local.md"),
        (vec!["common"], "general", "carabiner.local.jsonc"),
        (vec!["common"], "general", ".carabiner.local/"),
        (vec!["common"], "general", "**/AGENTS.local.md"),
        (vec!["claudecode"], "rules", "**/CLAUDE.local.md"),
        (vec!["claudecode"], "rules", "**/.claude/CLAUDE.local.md"),
        (vec!["qwencode"], "rules", "**/.qwen/QWEN.local.md"),
        (vec!["vibe"], "subagents", "**/.vibe/prompts/"),
        (vec!["claudecode"], "general", "**/.claude/*.lock"),
        (
            vec!["claudecode"],
            "general",
            "**/.claude/settings.local.json",
        ),
        (vec!["claudecode"], "general", "**/.claude/memories/"),
        (
            vec!["opencode"],
            "general",
            "**/.opencode/package-lock.json",
        ),
        (vec!["devin"], "mcp", "**/.devin/mcp_config.local.json"),
        (vec!["rovodev"], "general", "**/.rovodev/.carabiner/"),
        (vec!["takt"], "general", "**/.takt/runs/"),
        (vec!["takt"], "general", "**/.takt/tasks/"),
        (vec!["takt"], "general", "**/.takt/.cache/"),
        (vec!["takt"], "general", "**/.takt/config.yaml"),
        (vec!["musecode"], "general", "**/.muse/worktrees/"),
        (vec!["augmentcode"], "rules", "**/.augment-guidelines"),
        (vec!["devin"], "commands", "**/.devin/workflows/"),
        (vec!["junie"], "rules", "**/.junie/memories/"),
        (vec!["goose"], "ignore", "**/.gooseignore"),
        (vec!["goose"], "subagents", "**/.goose/recipes/subagents/"),
        (vec!["agentsmd"], "subagents", "**/.agents/subagents/"),
        (vec!["junie"], "permissions", "**/.junie/allowlist.json"),
        (vec!["rovodev"], "skills", "**/.agents/skills/"),
        (vec!["rovodev"], "commands", "**/.rovodev/prompts.yml"),
        (vec!["devin"], "skills", "**/.config/devin/skills/"),
        (vec!["copilotcli"], "subagents", "**/.copilot/agents/"),
        (vec!["copilotcli"], "mcp", "**/.copilot/mcp-config.json"),
        (vec!["copilotcli"], "hooks", "**/.copilot/hooks/"),
        (
            vec!["hermesagent"],
            "ignore",
            "**/.hermes/plugins/carabiner-ignore/",
        ),
        (
            vec!["hermesagent"],
            "checks",
            "**/.hermes/plugins/carabiner-checks/",
        ),
        (vec!["cline"], "hooks", "**/.clinerules/hooks/Notification"),
        (
            vec!["cline"],
            "hooks",
            "**/.clinerules/hooks/Notification.ps1",
        ),
        (vec!["cline"], "hooks", "**/.clinerules/hooks/PostToolUse"),
        (
            vec!["cline"],
            "hooks",
            "**/.clinerules/hooks/PostToolUse.ps1",
        ),
        (vec!["cline"], "hooks", "**/.clinerules/hooks/PreCompact"),
        (
            vec!["cline"],
            "hooks",
            "**/.clinerules/hooks/PreCompact.ps1",
        ),
        (vec!["cline"], "hooks", "**/.clinerules/hooks/PreToolUse"),
        (
            vec!["cline"],
            "hooks",
            "**/.clinerules/hooks/PreToolUse.ps1",
        ),
        (
            vec!["cline"],
            "hooks",
            "**/.clinerules/hooks/SessionShutdown",
        ),
        (
            vec!["cline"],
            "hooks",
            "**/.clinerules/hooks/SessionShutdown.ps1",
        ),
        (vec!["cline"], "hooks", "**/.clinerules/hooks/TaskComplete"),
        (
            vec!["cline"],
            "hooks",
            "**/.clinerules/hooks/TaskComplete.ps1",
        ),
        (vec!["cline"], "hooks", "**/.clinerules/hooks/TaskError"),
        (vec!["cline"], "hooks", "**/.clinerules/hooks/TaskError.ps1"),
        (vec!["cline"], "hooks", "**/.clinerules/hooks/TaskStart"),
        (vec!["cline"], "hooks", "**/.clinerules/hooks/TaskStart.ps1"),
        (
            vec!["cline"],
            "hooks",
            "**/.clinerules/hooks/UserPromptSubmit",
        ),
        (
            vec!["cline"],
            "hooks",
            "**/.clinerules/hooks/UserPromptSubmit.ps1",
        ),
        (vec!["roo"], "subagents", "**/.roomodes"),
        (vec!["codexcli"], "ignore", "**/.codexignore"),
        (
            vec!["codexcli"],
            "permissions",
            "**/.codex/rules/carabiner.rules",
        ),
    ]
}

fn update_ignore_file(path: &Path, entries: &[String], known_entries: &[String]) -> Result<bool> {
    const HEADER: &str = "# Generated by Carabiner";
    const FOOTER: &str = "# End of Carabiner";
    const LEGACY_HEADER: &str = "# Generated by carabiner - AI tool configuration files";
    let project_root = std::env::current_dir()?
        .canonicalize()
        .unwrap_or(std::env::current_dir()?);
    let target = if path.is_absolute() {
        path.to_path_buf()
    } else {
        project_root.join(path)
    };
    assert_no_symlink_path(&project_root, &target)?;
    let old = if path.exists() {
        fs::read_to_string(path)?
    } else {
        String::new()
    };
    let is_known = |line: &str| known_entries.iter().any(|entry| entry == line.trim());
    let lines = old.split('\n').collect::<Vec<_>>();
    let mut kept = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        if line.trim() == HEADER || line.trim() == LEGACY_HEADER {
            let mut footer = None;
            for (candidate, candidate_line) in lines.iter().enumerate().skip(index + 1) {
                if candidate_line.trim() == FOOTER {
                    footer = Some(candidate);
                    break;
                }
                if candidate_line.trim() == HEADER || candidate_line.trim() == LEGACY_HEADER {
                    break;
                }
            }
            if let Some(footer) = footer {
                index = footer + 1;
                continue;
            }
            index += 1;
            let mut empty_lines = 0;
            while index < lines.len() {
                let candidate = lines[index];
                if candidate.trim().is_empty() {
                    empty_lines += 1;
                    index += 1;
                    if empty_lines >= 2 {
                        break;
                    }
                } else if is_known(candidate) {
                    empty_lines = 0;
                    index += 1;
                } else {
                    break;
                }
            }
            continue;
        }
        if is_known(line) {
            index += 1;
            continue;
        }
        kept.push(line.to_owned());
        index += 1;
    }
    let mut content = kept.join("\n").trim_end().to_owned();
    if !entries.is_empty() {
        if !content.is_empty() {
            content.push_str("\n\n");
        }
        content.push_str(HEADER);
        content.push('\n');
        content.push_str(&entries.join("\n"));
        content.push('\n');
        content.push_str(FOOTER);
    }
    let next = if content.is_empty() {
        String::new()
    } else {
        format!("{content}\n")
    };
    if old == next {
        return Ok(false);
    }
    if next.is_empty() {
        if path.exists() {
            fs::remove_file(path)?;
        }
    } else {
        write_text(path, &next, false)?;
    }
    Ok(true)
}

fn fetch_command(options: &FetchCli) -> Result<Value> {
    if options.interactive && (!io::stdin().is_terminal() || !io::stdout().is_terminal()) {
        return Err(anyhow!(
            "The --interactive option requires an interactive terminal (TTY). Use --skills <names> to select skills non-interactively."
        ));
    }
    if !matches!(options.conflict.as_str(), "overwrite" | "skip") {
        return Err(anyhow!(
            "Invalid conflict strategy '{}'. Expected one of: skip, overwrite.",
            options.conflict
        ));
    }
    let mut features =
        parse_list(options.features.as_ref()).unwrap_or_else(|| vec!["skills".into()]);
    if features.iter().any(|feature| feature == "*") {
        features = all_features();
    }
    for feature in &features {
        if !all_features().iter().any(|known| known == feature) {
            return Err(anyhow!("Invalid feature '{feature}'."));
        }
    }
    let target = options.target.as_deref().unwrap_or("carabiner");
    if target != "carabiner" && target_spec(target).is_none() {
        return Err(anyhow!("Invalid fetch target '{target}'."));
    }
    if (options.skills.is_some() || options.interactive)
        && !features.iter().any(|feature| feature == "skills")
    {
        return Err(anyhow!(
            "The --skills and --interactive options require the skills feature. Add 'skills' to --features or omit --features to use the default."
        ));
    }
    let output = PathBuf::from(
        options
            .output
            .clone()
            .unwrap_or_else(|| ".carabiner".into()),
    );
    if output.is_absolute() {
        return Err(anyhow!("fetch output path must be relative"));
    }
    safe_relative_path(&output.to_string_lossy())?;
    let selected_skills = parse_list(options.skills.as_ref());
    let outcome = fetch_source_internal(
        &options.source,
        target,
        &features,
        selected_skills.as_deref(),
        options.reference.as_deref(),
        options.path.as_deref(),
        &output,
        &options.conflict,
        options.interactive,
    )?;
    let copied = outcome.total();
    if !options.silent && human_output() {
        println!("Fetched {copied} files");
        if copied == 0 {
            println!("No files were fetched.");
        }
    }
    let mut data = Map::from_iter([
        ("source".into(), Value::String(options.source.clone())),
        (
            "created".into(),
            Value::Array(outcome.created.iter().cloned().map(Value::String).collect()),
        ),
        (
            "overwritten".into(),
            Value::Array(
                outcome
                    .overwritten
                    .iter()
                    .cloned()
                    .map(Value::String)
                    .collect(),
            ),
        ),
        (
            "skipped".into(),
            Value::Array(outcome.skipped.iter().cloned().map(Value::String).collect()),
        ),
        ("totalFetched".into(), Value::Number(copied.into())),
    ]);
    if let Some(path) = options.path.as_ref() {
        data.insert("path".into(), Value::String(path.clone()));
    }
    Ok(Value::Object(data))
}

fn parse_fetch_remote_source(source: &str) -> Result<(String, Option<String>, Option<String>)> {
    let mut value = source.trim().to_owned();
    if let Some(rest) = value.strip_prefix("github:") {
        value = rest.to_owned();
    } else if value.strip_prefix("gitlab:").is_some() {
        return Err(anyhow!(
            "GitLab is not yet supported. Currently only GitHub repositories are supported."
        ));
    } else if value.starts_with("http://") || value.starts_with("https://") {
        let without_scheme = value
            .split_once("://")
            .map(|(_, rest)| rest)
            .unwrap_or(value.as_str());
        let mut segments = without_scheme.split('/');
        let host = segments.next().unwrap_or("").to_ascii_lowercase();
        if host == "gitlab.com" || host == "www.gitlab.com" {
            return Err(anyhow!(
                "GitLab is not yet supported. Currently only GitHub repositories are supported."
            ));
        }
        let owner = segments.next().unwrap_or("");
        let repo = segments.next().unwrap_or("").trim_end_matches(".git");
        if owner.is_empty() || repo.is_empty() {
            return Err(anyhow!("Invalid repository URL: {source}"));
        }
        let rest = segments.collect::<Vec<_>>();
        if rest.first().copied() == Some("tree") || rest.first().copied() == Some("blob") {
            let inferred_ref = rest
                .get(1)
                .filter(|value| !value.is_empty())
                .map(|v| (*v).to_owned());
            let inferred_path = (rest.len() > 2).then(|| rest[2..].join("/"));
            return Ok((
                format!("https://github.com/{owner}/{repo}.git"),
                inferred_ref,
                inferred_path,
            ));
        }
        return Ok((value, None, None));
    }
    let mut inferred_path = None;
    if let Some((repo, path)) = value.split_once(':') {
        if !path.is_empty() {
            inferred_path = Some(path.to_owned());
            value = repo.to_owned();
        }
    }
    let mut inferred_ref = None;
    if let Some((repo, reference)) = value
        .rsplit_once('@')
        .map(|(repo, reference)| (repo.to_owned(), reference.to_owned()))
    {
        if !repo.is_empty() && !reference.is_empty() && repo.contains('/') {
            value = repo;
            inferred_ref = Some(reference);
        }
    }
    let mut parts = value.split('/');
    let owner = parts.next().unwrap_or("");
    let repo = parts.next().unwrap_or("").trim_end_matches(".git");
    if owner.is_empty() || repo.is_empty() || parts.next().is_some() {
        return Err(anyhow!(
            "Invalid source: {source}. Expected format: owner/repo, owner/repo@ref, or owner/repo:path"
        ));
    }
    Ok((
        format!("https://github.com/{owner}/{repo}.git"),
        inferred_ref,
        inferred_path,
    ))
}

struct TempDirGuard(Vec<PathBuf>);

impl TempDirGuard {
    fn new(path: Option<PathBuf>) -> Self {
        Self(path.into_iter().collect())
    }

    fn add(&mut self, path: PathBuf) {
        self.0.push(path);
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        for path in self.0.drain(..) {
            if fs::remove_dir_all(&path).is_err() {
                let _ = fs::remove_file(path);
            }
        }
    }
}

fn available_skill_names(root: &Path) -> Vec<String> {
    let mut names = HashSet::new();
    for candidate in [root.join("skills"), root.join(".carabiner/skills")] {
        if !candidate.is_dir() {
            continue;
        }
        for path in direct_dirs(&candidate) {
            if let Some(name) = path.file_name().and_then(|value| value.to_str()) {
                names.insert(name.to_owned());
            }
        }
        if !names.is_empty() {
            break;
        }
    }
    let mut names = names.into_iter().collect::<Vec<_>>();
    names.sort();
    names
}

fn prompt_skill_selection(
    root: &Path,
    preselected_skills: Option<&[String]>,
) -> Result<Vec<String>> {
    let available = available_skill_names(root);
    if available.is_empty() {
        eprintln!("No skills found in the source repository to select from.");
        return Ok(Vec::new());
    }
    let preselected = preselected_skills.unwrap_or_default();
    let unknown = preselected
        .iter()
        .filter(|name| !available.contains(name))
        .cloned()
        .collect::<Vec<_>>();
    if !unknown.is_empty() {
        return Err(anyhow!(
            "Unknown skill(s): {}. Available skills: {}",
            unknown.join(", "),
            available.join(", ")
        ));
    }
    println!("Select skills to fetch (comma-separated numbers; empty keeps the defaults):");
    for (index, name) in available.iter().enumerate() {
        let selected = preselected.is_empty() || preselected.iter().any(|value| value == name);
        println!(
            "  [{}] {}: {}",
            if selected { "x" } else { " " },
            index + 1,
            name
        );
    }
    print!("Selection: ");
    io::stdout().flush()?;
    let mut input = String::new();
    if io::stdin().read_line(&mut input)? == 0 {
        return Ok(Vec::new());
    }
    let input = input.trim();
    if input.is_empty() {
        return Ok(if preselected.is_empty() {
            available
        } else {
            preselected.to_vec()
        });
    }
    let mut selected = HashSet::new();
    for token in input
        .split(',')
        .map(str::trim)
        .filter(|token| !token.is_empty())
    {
        let name = if let Ok(index) = token.parse::<usize>() {
            available
                .get(
                    index
                        .checked_sub(1)
                        .ok_or_else(|| anyhow!("Skill numbers start at 1"))?,
                )
                .cloned()
                .ok_or_else(|| anyhow!("Unknown skill selection '{token}'"))?
        } else if available.iter().any(|name| name == token) {
            token.to_owned()
        } else {
            return Err(anyhow!(
                "Unknown skill selection '{token}'. Available skills: {}",
                available.join(", ")
            ));
        };
        selected.insert(name);
    }
    Ok(available
        .into_iter()
        .filter(|name| selected.contains(name))
        .collect())
}

#[derive(Debug, Default)]
struct FetchOutcome {
    created: Vec<String>,
    overwritten: Vec<String>,
    skipped: Vec<String>,
}

impl FetchOutcome {
    fn total(&self) -> usize {
        self.created.len() + self.overwritten.len() + self.skipped.len()
    }

    fn extend(&mut self, other: Self) {
        self.created.extend(other.created);
        self.overwritten.extend(other.overwritten);
        self.skipped.extend(other.skipped);
    }
}

#[allow(clippy::too_many_arguments)]
fn fetch_source_internal(
    source: &str,
    target: &str,
    features: &[String],
    selected_skills: Option<&[String]>,
    reference: Option<&str>,
    subpath: Option<&str>,
    output: &Path,
    conflict: &str,
    interactive: bool,
) -> Result<FetchOutcome> {
    let cwd = std::env::current_dir()?;
    if output.is_absolute() {
        assert_no_symlink_path(output, output)?;
    } else {
        assert_no_symlink_path(Path::new(""), output)?;
    }
    let source_path = PathBuf::from(source);
    let (root, temporary, _inferred_ref, inferred_path) = if source_path.is_dir() {
        (
            if source_path.is_absolute() {
                source_path
            } else {
                cwd.join(source_path)
            },
            None,
            None,
            None,
        )
    } else {
        let (url, source_ref, source_path) = parse_fetch_remote_source(source)?;
        let inferred_ref = reference.map(ToOwned::to_owned).or(source_ref);
        if inferred_ref
            .as_deref()
            .is_some_and(|value| value.starts_with('-'))
        {
            return Err(anyhow!("fetch ref must not start with '-'"));
        }
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let temp =
            std::env::temp_dir().join(format!("carabiner-fetch-{}-{stamp}", std::process::id()));
        let mut command = Command::new("git");
        command.arg("clone").arg("--depth").arg("1");
        if let Some(reference) = inferred_ref.as_deref() {
            command.arg("--branch").arg(reference);
        }
        command.arg(&url).arg(&temp);
        let status = command.status().context("failed to start git")?;
        if !status.success() {
            let _ = fs::remove_dir_all(&temp);
            return Err(anyhow!("git clone failed for {source}"));
        }
        (temp.clone(), Some(temp), inferred_ref, source_path)
    };
    let mut temporary_cleanup = TempDirGuard::new(temporary.clone());
    let requested_path = subpath.map(ToOwned::to_owned).or(inferred_path);
    let root = if let Some(subpath) = requested_path {
        safe_relative_path(&subpath)?;
        root.join(subpath)
    } else {
        root
    };
    if !root.is_dir() {
        if let Some(temp) = temporary {
            let _ = fs::remove_dir_all(temp);
        }
        return Err(anyhow!(
            "fetch source path does not exist: {}",
            root.display()
        ));
    }
    let interactive_selection = if interactive && features.iter().any(|feature| feature == "skills")
    {
        Some(prompt_skill_selection(&root, selected_skills)?)
    } else {
        None
    };
    let selected_skills = interactive_selection.as_deref().or(selected_skills);
    let mut outcome = FetchOutcome::default();
    if target == "carabiner" {
        for feature in features {
            let source_feature = match feature.as_str() {
                "skills" => first_existing(&[root.join("skills"), root.join(".carabiner/skills")]),
                "rules" => first_existing(&[root.join("rules"), root.join(".carabiner/rules")]),
                "commands" => {
                    first_existing(&[root.join("commands"), root.join(".carabiner/commands")])
                }
                "subagents" => {
                    first_existing(&[root.join("subagents"), root.join(".carabiner/subagents")])
                }
                "checks" => first_existing(&[root.join("checks"), root.join(".carabiner/checks")]),
                "mcp" => {
                    first_existing(&[root.join("mcp.jsonc"), root.join(".carabiner/mcp.jsonc")])
                }
                "hooks" => first_existing(&[
                    root.join("hooks.jsonc"),
                    root.join(".carabiner/hooks.jsonc"),
                ]),
                "permissions" => first_existing(&[
                    root.join("permissions.jsonc"),
                    root.join(".carabiner/permissions.jsonc"),
                ]),
                "ignore" => {
                    first_existing(&[root.join(".aiignore"), root.join(".carabiner/.aiignore")])
                }
                _ => None,
            };
            let Some(source_feature) = source_feature else {
                continue;
            };
            let destination = match feature.as_str() {
                "skills" => output.join("skills"),
                "rules" => output.join("rules"),
                "commands" => output.join("commands"),
                "subagents" => output.join("subagents"),
                "checks" => output.join("checks"),
                _ => output.join(
                    source_feature
                        .file_name()
                        .and_then(|v| v.to_str())
                        .unwrap_or(feature),
                ),
            };
            if source_feature.is_dir() {
                if feature == "skills" {
                    if let Some(selected) = selected_skills {
                        let available = direct_dirs(&source_feature)
                            .into_iter()
                            .filter_map(|path| {
                                path.file_name()
                                    .and_then(|value| value.to_str())
                                    .map(ToOwned::to_owned)
                            })
                            .collect::<HashSet<_>>();
                        let unknown = selected
                            .iter()
                            .filter(|name| !available.contains(*name))
                            .cloned()
                            .collect::<Vec<_>>();
                        if !unknown.is_empty() {
                            let mut names = available.into_iter().collect::<Vec<_>>();
                            names.sort();
                            return Err(anyhow!(
                                "Unknown skill(s): {}. Available skills: {}",
                                unknown.join(", "),
                                if names.is_empty() {
                                    "(no skills found)".to_owned()
                                } else {
                                    names.join(", ")
                                }
                            ));
                        }
                        for skill_dir in direct_dirs(&source_feature) {
                            let name = skill_dir
                                .file_name()
                                .and_then(|value| value.to_str())
                                .unwrap_or("");
                            if selected.iter().any(|wanted| wanted == name) {
                                outcome.extend(copy_tree_for_fetch(
                                    &skill_dir,
                                    &destination.join(name),
                                    &format!("skills/{name}"),
                                    conflict,
                                )?);
                            }
                        }
                    } else {
                        outcome.extend(copy_tree_for_fetch(
                            &source_feature,
                            &destination,
                            "skills",
                            conflict,
                        )?);
                    }
                } else {
                    outcome.extend(copy_tree_for_fetch(
                        &source_feature,
                        &destination,
                        feature,
                        conflict,
                    )?);
                }
            } else {
                let relative = source_feature
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or(feature)
                    .to_owned();
                outcome.extend(copy_file_for_fetch(
                    &source_feature,
                    &destination,
                    &relative,
                    conflict,
                )?);
            }
        }
    } else {
        let conversion_features = features
            .iter()
            .filter(|feature| feature.as_str() != "skills")
            .cloned()
            .collect::<Vec<_>>();
        if !conversion_features.is_empty() {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let staging = std::env::temp_dir().join(format!(
                "carabiner-fetch-convert-{}-{stamp}",
                std::process::id()
            ));
            fs::create_dir_all(&staging)?;
            temporary_cleanup.add(staging.clone());
            export_canonical_to_tool_directory(target, &root, &staging, &conversion_features)?;
            let before = snapshot_fetch_files(output);
            let result =
                import_tool_to_directory(target, &staging, output, &conversion_features, false)?;
            let after = snapshot_fetch_files(output);
            outcome = classify_fetch_changes(&before, &after);
            if outcome.total() == 0 && result.total_files() > 0 {
                return Err(anyhow!(
                    "Native fetch wrote files but could not classify its output paths"
                ));
            }
        }
    }
    Ok(outcome)
}

fn first_existing(paths: &[PathBuf]) -> Option<PathBuf> {
    paths.iter().find(|path| path.exists()).cloned()
}

fn is_never_carried(relative: &Path) -> bool {
    relative.components().any(|component| {
        let name = component.as_os_str().to_string_lossy().to_ascii_lowercase();
        matches!(
            name.as_str(),
            ".git"
                | ".hg"
                | ".svn"
                | ".cache"
                | ".venv"
                | ".tox"
                | ".next"
                | ".ds_store"
                | ".npmrc"
                | ".netrc"
                | ".env"
                | ".envrc"
        )
    })
}

fn copy_tree(source: &Path, destination: &Path, conflict: &str) -> Result<usize> {
    let mut copied = 0;
    for path in walk_files(source) {
        let relative = path.strip_prefix(source).unwrap_or(&path);
        if is_never_carried(relative) {
            continue;
        }
        if fs::metadata(&path)?.len() > 10 * 1024 * 1024 {
            continue;
        }
        let target = destination.join(relative);
        assert_no_symlink_path(destination, &target)?;
        if target.exists() {
            match conflict {
                "skip" => continue,
                "error" => {
                    return Err(anyhow!(
                        "Refusing to overwrite existing file {}",
                        target.display()
                    ))
                }
                _ => {}
            }
        }
        if write_bytes(&target, &fs::read(&path)?, false)? {
            copied += 1;
        }
    }
    Ok(copied)
}
fn copy_tree_for_fetch(
    source: &Path,
    destination: &Path,
    prefix: &str,
    conflict: &str,
) -> Result<FetchOutcome> {
    let mut outcome = FetchOutcome::default();
    for path in walk_files(source) {
        let relative = path.strip_prefix(source).unwrap_or(&path);
        if is_never_carried(relative) || fs::metadata(&path)?.len() > 10 * 1024 * 1024 {
            continue;
        }
        let target = destination.join(relative);
        let relative_path = format!("{prefix}/{}", relative.to_string_lossy().replace('\\', "/"));
        assert_no_symlink_path(destination, &target)?;
        let was_existing = target.exists();
        if was_existing {
            match conflict {
                "skip" => {
                    outcome.skipped.push(relative_path);
                    continue;
                }
                "error" => {
                    return Err(anyhow!(
                        "Refusing to overwrite existing file {}",
                        target.display()
                    ));
                }
                _ => {}
            }
        }
        write_bytes(&target, &fs::read(&path)?, false)?;
        if was_existing {
            outcome.overwritten.push(relative_path);
        } else {
            outcome.created.push(relative_path);
        }
    }
    Ok(outcome)
}

fn copy_file_for_fetch(
    source: &Path,
    destination: &Path,
    relative_path: &str,
    conflict: &str,
) -> Result<FetchOutcome> {
    let mut outcome = FetchOutcome::default();
    let was_existing = destination.exists();
    if was_existing {
        match conflict {
            "skip" => {
                outcome.skipped.push(relative_path.to_owned());
                return Ok(outcome);
            }
            "error" => {
                return Err(anyhow!(
                    "Refusing to overwrite existing file {}",
                    destination.display()
                ));
            }
            _ => {}
        }
    }
    write_bytes(destination, &fs::read(source)?, false)?;
    if was_existing {
        outcome.overwritten.push(relative_path.to_owned());
    } else {
        outcome.created.push(relative_path.to_owned());
    }
    Ok(outcome)
}

fn snapshot_fetch_files(root: &Path) -> HashMap<String, Vec<u8>> {
    if !root.is_dir() {
        return HashMap::new();
    }
    walk_files(root)
        .into_iter()
        .filter_map(|path| {
            let relative = relative_slash(root, &path);
            fs::read(path).ok().map(|content| (relative, content))
        })
        .collect()
}

fn classify_fetch_changes(
    before: &HashMap<String, Vec<u8>>,
    after: &HashMap<String, Vec<u8>>,
) -> FetchOutcome {
    let mut paths = after.keys().cloned().collect::<Vec<_>>();
    paths.sort();
    let mut outcome = FetchOutcome::default();
    for path in paths {
        match before.get(&path) {
            None => outcome.created.push(path),
            Some(previous) if after.get(&path) != Some(previous) => outcome.overwritten.push(path),
            Some(_) => {}
        }
    }
    outcome
}

fn normalized_source_key(source: &str) -> String {
    let mut key = source.trim().to_owned();
    for prefix in [
        "https://www.github.com/",
        "https://github.com/",
        "http://www.github.com/",
        "http://github.com/",
        "https://www.gitlab.com/",
        "https://gitlab.com/",
        "http://www.gitlab.com/",
        "http://gitlab.com/",
    ] {
        if key.to_ascii_lowercase().starts_with(prefix) {
            key = key[prefix.len()..].to_owned();
            break;
        }
    }
    for prefix in ["github:", "gitlab:"] {
        if key.starts_with(prefix) {
            key = key[prefix.len()..].to_owned();
            break;
        }
    }
    key.trim_end_matches('/')
        .trim_end_matches(".git")
        .to_ascii_lowercase()
}

fn read_sources_lock(path: &Path) -> Map<String, Value> {
    read_jsonc(path)
        .ok()
        .and_then(|value| value.get("sources").and_then(Value::as_object).cloned())
        .unwrap_or_default()
}

fn write_sources_lock(path: &Path, sources: &Map<String, Value>) -> Result<()> {
    if sources.is_empty() {
        if path.is_file() {
            fs::remove_file(path)?;
        }
        return Ok(());
    }
    let value = Value::Object(Map::from_iter([
        ("lockfileVersion".into(), Value::Number(1.into())),
        ("sources".into(), Value::Object(sources.clone())),
    ]));
    write_text(path, &json_pretty(&value)?, false)?;
    Ok(())
}

fn sha256_bytes(content: &[u8]) -> String {
    let mut command = Command::new("shasum");
    command.args(["-a", "256"]);
    let output = command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            if let Some(mut stdin) = child.stdin.take() {
                use std::io::Write as _;
                stdin.write_all(content)?;
            }
            child.wait_with_output()
        });
    if let Ok(output) = output {
        if output.status.success() {
            if let Some(hash) = String::from_utf8_lossy(&output.stdout)
                .split_whitespace()
                .next()
            {
                if hash.len() == 64 {
                    return format!("sha256-{hash}");
                }
            }
        }
    }
    "sha256-".to_owned() + &"0".repeat(64)
}

fn git_revision(source: &str, requested_ref: Option<&str>) -> String {
    let source_path = PathBuf::from(source);
    if source_path.is_dir() {
        if let Ok(output) = Command::new("git")
            .args(["-C", &source_path.to_string_lossy(), "rev-parse", "HEAD"])
            .output()
        {
            if output.status.success() {
                let revision = String::from_utf8_lossy(&output.stdout).trim().to_owned();
                if revision.len() == 40 {
                    return revision;
                }
            }
        }
    } else if let Ok((url, _, _)) = parse_fetch_remote_source(source) {
        let mut command = Command::new("git");
        command.args(["ls-remote", &url]);
        if let Some(reference) = requested_ref {
            command.arg(reference);
        } else {
            command.arg("HEAD");
        }
        if let Ok(output) = command.output() {
            if output.status.success() {
                if let Some(revision) = String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().next())
                {
                    if revision.len() == 40 {
                        return revision.to_owned();
                    }
                }
            }
        }
    }
    "0".repeat(40)
}

fn source_string_list(object: &Map<String, Value>, key: &str) -> Option<Vec<String>> {
    object.get(key).map(|value| {
        value
            .as_array()
            .map_or(&[] as &[Value], Vec::as_slice)
            .iter()
            .filter_map(Value::as_str)
            .map(ToOwned::to_owned)
            .collect()
    })
}

fn copy_source_skills(
    source: &Path,
    destination: &Path,
    selected: Option<&[String]>,
    blocked: &HashSet<String>,
) -> Result<(usize, HashSet<String>)> {
    let mut copied = 0usize;
    let mut names = HashSet::new();
    for directory in direct_dirs(source) {
        let Some(name) = directory.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if name == ".curated"
            || selected
                .is_some_and(|values| !values.iter().any(|wanted| wanted == "*" || wanted == name))
            || blocked.contains(name)
            || !directory.join("SKILL.md").is_file()
        {
            continue;
        }
        copied += copy_tree(&directory, &destination.join(name), "overwrite")?;
        names.insert(name.to_owned());
    }
    if names.is_empty() && source.join("SKILL.md").is_file() {
        let name = source
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("skill");
        if !blocked.contains(name)
            && selected
                .is_none_or(|values| values.iter().any(|wanted| wanted == "*" || wanted == name))
        {
            copied += copy_tree(source, &destination.join(name), "overwrite")?;
            names.insert(name.to_owned());
        }
    }
    Ok((copied, names))
}

fn copy_selected_rules_excluding(
    source: &Path,
    destination: &Path,
    selected: Option<&[String]>,
    blocked: &HashSet<String>,
) -> Result<(usize, HashSet<String>)> {
    let mut copied = 0usize;
    let mut names = HashSet::new();
    for path in walk_files(source) {
        let relative = path.strip_prefix(source).unwrap_or(&path);
        let relative_string = relative.to_string_lossy().replace('\\', "/");
        let name = relative_string
            .strip_suffix(".md")
            .unwrap_or(&relative_string);
        if let Some(selected) = selected {
            if !selected
                .iter()
                .any(|wanted| wanted == "*" || wanted == name)
            {
                continue;
            }
        }
        if blocked.contains(name) || name.is_empty() || name.contains("..") {
            continue;
        }
        if fs::metadata(&path)?.len() > 10 * 1024 * 1024 {
            continue;
        }
        safe_relative_path(&relative_string)?;
        let target = destination.join(relative);
        assert_no_symlink_path(destination, &target)?;
        if write_bytes(&target, &fs::read(&path)?, false)? {
            copied += 1;
        }
        names.insert(name.to_owned());
    }
    Ok((copied, names))
}

fn lock_entry_names(entry: &Value, key: &str) -> HashSet<String> {
    entry
        .get(key)
        .and_then(Value::as_object)
        .map(|values| values.keys().cloned().collect())
        .unwrap_or_default()
}

fn restrict_source_lock_entry(
    mut entry: Value,
    skill_names: &HashSet<String>,
    rule_names: &HashSet<String>,
) -> Value {
    if let Some(object) = entry.as_object_mut() {
        for (key, allowed) in [("skills", skill_names), ("rules", rule_names)] {
            if let Some(Value::Object(values)) = object.get_mut(key) {
                values.retain(|name, _| allowed.contains(name));
            }
        }
    }
    entry
}
fn source_lock_skill_entry(path: &Path) -> Result<Value> {
    let mut payload = Vec::new();
    for file in walk_files(path) {
        let relative = relative_slash(path, &file);
        payload.extend_from_slice(relative.as_bytes());
        payload.push(0);
        payload.extend_from_slice(&fs::read(&file)?);
        payload.push(0);
    }
    Ok(json!({"integrity": sha256_bytes(&payload)}))
}

fn source_lock_rules(curated: &Path, selected: Option<&[String]>) -> Result<Map<String, Value>> {
    let mut rules = Map::new();
    for path in walk_files(curated) {
        if path.extension().and_then(|value| value.to_str()) != Some("md") {
            continue;
        }
        let relative = relative_slash(curated, &path);
        let name = relative.strip_suffix(".md").unwrap_or(&relative);
        if let Some(selected) = selected {
            if !selected
                .iter()
                .any(|wanted| wanted == "*" || wanted == name)
            {
                continue;
            }
        }
        let content = fs::read(&path)?;
        rules.insert(
            name.to_owned(),
            json!({"integrity": sha256_bytes(&content)}),
        );
    }
    Ok(rules)
}

fn source_lock_entry(
    source: &str,
    requested_ref: Option<&str>,
    selected_skills: Option<&[String]>,
    selected_rules: Option<&[String]>,
    rules_path: Option<&str>,
    curated_skills: &Path,
    curated_rules: &Path,
) -> Result<Value> {
    let mut skills = Map::new();
    for directory in direct_dirs(curated_skills) {
        let main = directory.join("SKILL.md");
        if !main.is_file() {
            continue;
        }
        let name = directory
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        if let Some(selected) = selected_skills {
            if !selected
                .iter()
                .any(|wanted| wanted == "*" || wanted == name)
            {
                continue;
            }
        }
        skills.insert(name.to_owned(), source_lock_skill_entry(&directory)?);
    }
    let mut entry = Map::from_iter([
        (
            "resolvedRef".into(),
            Value::String(git_revision(source, requested_ref)),
        ),
        (
            "resolvedAt".into(),
            Value::String(chrono::Utc::now().to_rfc3339()),
        ),
        ("skills".into(), Value::Object(skills)),
    ]);
    if let Some(requested_ref) = requested_ref {
        entry.insert(
            "requestedRef".into(),
            Value::String(requested_ref.to_owned()),
        );
    }
    if selected_rules.is_some() {
        let rules = source_lock_rules(curated_rules, selected_rules)?;
        entry.insert("rules".into(), Value::Object(rules));
        if let Some(selected_rules) = selected_rules {
            entry.insert(
                "ruleSelection".into(),
                Value::Array(selected_rules.iter().cloned().map(Value::String).collect()),
            );
        }
        if let Some(rules_path) = rules_path {
            entry.insert("rulesPath".into(), Value::String(rules_path.to_owned()));
        }
    }
    Ok(Value::Object(entry))
}

fn read_npm_lock(path: &Path) -> Map<String, Value> {
    read_jsonc(path)
        .ok()
        .and_then(|value| value.get("sources").and_then(Value::as_object).cloned())
        .unwrap_or_default()
}

fn write_npm_lock(path: &Path, sources: &Map<String, Value>) -> Result<()> {
    if sources.is_empty() {
        if path.is_file() {
            fs::remove_file(path)?;
        }
        return Ok(());
    }
    let value = Value::Object(Map::from_iter([
        ("lockfileVersion".into(), Value::Number(1.into())),
        ("sources".into(), Value::Object(sources.clone())),
    ]));
    write_text(path, &json_pretty(&value)?, false)?;
    Ok(())
}

fn valid_npm_package_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 214 {
        return false;
    }
    let package = name.strip_prefix('@').unwrap_or(name);
    let mut parts = package.split('/');
    let (scope, package_name) = if name.starts_with('@') {
        (parts.next(), parts.next())
    } else {
        (None, parts.next())
    };
    if parts.next().is_some() || package_name.is_none() {
        return false;
    }
    let valid_part = |part: &str| {
        !part.is_empty()
            && part
                .chars()
                .next()
                .is_some_and(|value| value.is_ascii_alphanumeric())
            && part.chars().all(|value| {
                value.is_ascii_alphanumeric() || matches!(value, '.' | '_' | '-' | '~')
            })
    };
    scope.is_none_or(valid_part) && valid_part(package_name.unwrap_or(""))
}

fn url_origin(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let authority = rest.split('/').next()?.to_ascii_lowercase();
    Some(format!("{}://{authority}", scheme.to_ascii_lowercase()))
}

fn npm_request(url: &str, token: Option<&str>, accept: &str) -> Result<Vec<u8>> {
    let mut command = Command::new("curl");
    command.args([
        "-sSL",
        "-H",
        &format!("Accept: {accept}"),
        "-w",
        "%{http_code}",
    ]);
    if let Some(token) = token {
        command.args(["-H", &format!("Authorization: Bearer {token}")]);
    }
    command.arg(url);
    let output = command
        .output()
        .with_context(|| format!("failed to request npm registry URL {url}"))?;
    if output.stdout.len() < 3 {
        return Err(anyhow!("npm registry request failed for {url}"));
    }
    let status_start = output.stdout.len() - 3;
    let status = String::from_utf8_lossy(&output.stdout[status_start..]);
    let status = status
        .parse::<u16>()
        .map_err(|_| anyhow!("npm registry returned an invalid HTTP status for {url}"))?;
    if !output.status.success() || status >= 400 {
        if status == 401 || status == 403 {
            return Err(anyhow!(
                "HTTP {status} for {url}. Check tokenEnv or NPM_TOKEN."
            ));
        }
        return Err(anyhow!("HTTP {status} for {url}"));
    }
    Ok(output.stdout[..status_start].to_vec())
}

fn npm_integrity(path: &Path, integrity: &str) -> Result<()> {
    if integrity.is_empty() {
        return Ok(());
    }
    let (algorithm, expected) = integrity
        .split_once('-')
        .ok_or_else(|| anyhow!("Invalid npm integrity value"))?;
    if !matches!(algorithm, "sha512" | "sha384" | "sha256" | "sha1") {
        return Err(anyhow!("Unsupported npm integrity algorithm '{algorithm}'"));
    }
    if expected.len() == if algorithm == "sha1" { 40 } else { 64 }
        && expected.chars().all(|value| value.is_ascii_hexdigit())
    {
        let bits = match algorithm {
            "sha1" => "1",
            "sha256" => "256",
            "sha384" => "384",
            _ => "512",
        };
        let output = Command::new("shasum")
            .args(["-a", bits, path.to_string_lossy().as_ref()])
            .output()
            .context("failed to start shasum")?;
        let actual = String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_owned();
        if !output.status.success() || !actual.eq_ignore_ascii_case(expected) {
            return Err(anyhow!(
                "Integrity verification failed for npm tarball {}",
                path.display()
            ));
        }
        return Ok(());
    }
    let digest_argument = format!("-{algorithm}");
    let digest = Command::new("openssl")
        .args([
            "dgst",
            &digest_argument,
            "-binary",
            path.to_string_lossy().as_ref(),
        ])
        .output()
        .context("failed to start openssl")?;
    if !digest.status.success() {
        return Err(anyhow!("failed to calculate npm tarball integrity"));
    }
    let encoded = Command::new("base64")
        .arg("-b")
        .arg("0")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            if let Some(mut stdin) = child.stdin.take() {
                use std::io::Write as _;
                stdin.write_all(&digest.stdout)?;
            }
            child.wait_with_output()
        })?;
    let actual = String::from_utf8_lossy(&encoded.stdout).trim().to_owned();
    if actual != expected {
        return Err(anyhow!(
            "Integrity verification failed for npm tarball {}",
            path.display()
        ));
    }
    Ok(())
}

fn npm_package_url(registry: &str, package: &str) -> String {
    let encoded = package.replace('/', "%2F");
    format!("{}/{}", registry.trim_end_matches('/'), encoded)
}

fn safe_archive_member(name: &str) -> bool {
    let normalized = name.replace('\\', "/");
    !normalized.starts_with('/')
        && !normalized.split('/').any(|segment| segment == "..")
        && !normalized.contains('\0')
}

fn npm_shasum_to_sri(shasum: &str) -> Result<String> {
    if shasum.len() != 40 || !shasum.chars().all(|value| value.is_ascii_hexdigit()) {
        return Err(anyhow!(
            "Malformed sha1 shasum in registry metadata: \"{shasum}\""
        ));
    }
    let mut bytes = Vec::with_capacity(20);
    for pair in shasum.as_bytes().chunks_exact(2) {
        let high = (pair[0] as char)
            .to_digit(16)
            .ok_or_else(|| anyhow!("Malformed sha1 shasum in registry metadata: \"{shasum}\""))?;
        let low = (pair[1] as char)
            .to_digit(16)
            .ok_or_else(|| anyhow!("Malformed sha1 shasum in registry metadata: \"{shasum}\""))?;
        bytes.push((high * 16 + low) as u8);
    }
    Ok(format!("sha1-{}", encode_base64(&bytes)))
}
fn install_npm_source(
    object: &Map<String, Value>,
    source: &str,
    project_root: &Path,
    curated_skills: &Path,
    curated_rules: &Path,
    options: &InstallCli,
) -> Result<(usize, usize, Value)> {
    if !valid_npm_package_name(source) {
        return Err(anyhow!(
            "Invalid npm package name: \"{source}\". Expected \"name\" or \"@scope/name\"."
        ));
    }
    let registry = object
        .get("registry")
        .and_then(Value::as_str)
        .unwrap_or("https://registry.npmjs.org");
    if !registry.starts_with("https://") && !registry.starts_with("http://") {
        return Err(anyhow!("registry must be an http(s) URL"));
    }
    let token = if let Some(token_env) = object.get("tokenEnv").and_then(Value::as_str) {
        let value = std::env::var(token_env)
            .ok()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow!("Environment variable \"{token_env}\" (from tokenEnv) is not set. Export it or remove the tokenEnv field."))?;
        Some(value)
    } else {
        options.token.clone().or_else(|| {
            std::env::var("NPM_TOKEN")
                .ok()
                .filter(|value| !value.is_empty())
        })
    };
    let packument_url = npm_package_url(registry, source);
    let packument: Value = serde_json::from_slice(&npm_request(
        &packument_url,
        token.as_deref(),
        "application/vnd.npm.install-v1+json",
    )?)
    .context("invalid npm registry metadata")?;
    let requested_version = object.get("ref").and_then(Value::as_str);
    let resolved_version = if let Some(requested) = requested_version {
        if packument
            .get("versions")
            .and_then(Value::as_object)
            .is_some_and(|versions| versions.contains_key(requested))
        {
            requested.to_owned()
        } else if let Some(tagged) = packument
            .get("dist-tags")
            .and_then(Value::as_object)
            .and_then(|tags| tags.get(requested))
            .and_then(Value::as_str)
        {
            tagged.to_owned()
        } else {
            return Err(anyhow!("Could not resolve \"{source}@{requested}\": not an exact published version or dist-tag."));
        }
    } else {
        packument
            .get("dist-tags")
            .and_then(Value::as_object)
            .and_then(|tags| tags.get("latest"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .ok_or_else(|| anyhow!("npm package '{source}' has no latest version"))?
    };
    let metadata = packument
        .get("versions")
        .and_then(Value::as_object)
        .and_then(|versions| versions.get(&resolved_version))
        .ok_or_else(|| anyhow!("npm package '{source}' has no version '{resolved_version}'"))?;
    let dist = metadata
        .get("dist")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("npm package '{source}' has no distribution metadata"))?;
    let tarball_url = dist
        .get("tarball")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("npm package '{source}' has no tarball URL"))?;
    if !tarball_url.starts_with("https://") && !tarball_url.starts_with("http://") {
        return Err(anyhow!(
            "Unsupported tarball URL: \"{tarball_url}\". Use https:// (or http://)."
        ));
    }
    let tarball = project_root.join(format!(
        ".carabiner-npm-{}-{}.tgz",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let mut temporary_cleanup = TempDirGuard::new(Some(tarball.clone()));
    let tarball_token = (url_origin(tarball_url) == url_origin(&packument_url))
        .then_some(token.as_deref())
        .flatten();
    let bytes = npm_request(tarball_url, tarball_token, "*/*")?;
    if bytes.len() > 100 * 1024 * 1024 {
        return Err(anyhow!("npm tarball exceeds maximum 100MB size"));
    }
    write_bytes(&tarball, &bytes, false)?;
    let declared_integrity = dist.get("integrity").and_then(Value::as_str);
    let declared_shasum = dist.get("shasum").and_then(Value::as_str);
    if let Some(integrity) = declared_integrity {
        npm_integrity(&tarball, integrity)?;
    } else if let Some(shasum) = declared_shasum {
        npm_integrity(&tarball, &format!("sha1-{shasum}"))?;
    }
    let integrity = if let Some(value) = declared_integrity {
        value.to_owned()
    } else if let Some(value) = declared_shasum {
        npm_shasum_to_sri(value)?
    } else {
        String::new()
    };
    let staging = project_root.join(format!(
        ".carabiner-npm-extract-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    temporary_cleanup.add(staging.clone());
    fs::create_dir_all(&staging)?;
    let listing = Command::new("tar")
        .args(["-tzf", tarball.to_string_lossy().as_ref()])
        .output()
        .context("failed to start tar")?;
    if !listing.status.success() {
        let _ = fs::remove_file(&tarball);
        let _ = fs::remove_dir_all(&staging);
        return Err(anyhow!("invalid npm package tarball"));
    }
    for member in String::from_utf8_lossy(&listing.stdout).lines() {
        if !safe_archive_member(member) {
            let _ = fs::remove_file(&tarball);
            let _ = fs::remove_dir_all(&staging);
            return Err(anyhow!(
                "npm package contains an unsafe archive path '{member}'"
            ));
        }
    }
    let extracted = Command::new("tar")
        .args([
            "-xzf",
            tarball.to_string_lossy().as_ref(),
            "-C",
            staging.to_string_lossy().as_ref(),
        ])
        .status()
        .context("failed to start tar")?;
    if !extracted.success() {
        let _ = fs::remove_file(&tarball);
        let _ = fs::remove_dir_all(&staging);
        return Err(anyhow!("failed to extract npm package"));
    }
    let package_root = if staging.join("package").is_dir() {
        staging.join("package")
    } else {
        staging.clone()
    };
    let skills_declared = source_string_list(object, "skills");
    let rules_declared = source_string_list(object, "rules");
    let mut skill_root = package_root.clone();
    if let Some(path) = object.get("path").and_then(Value::as_str) {
        safe_relative_path(path)?;
        skill_root = package_root.join(path);
    }
    if !skill_root.is_dir() {
        return Err(anyhow!(
            "npm package skill path does not exist: {}",
            skill_root.display()
        ));
    }
    if skill_root.join("skills").is_dir() {
        skill_root = skill_root.join("skills");
    } else if skill_root.join(".carabiner/skills").is_dir() {
        skill_root = skill_root.join(".carabiner/skills");
    }
    let mut skills = 0usize;
    let mut installed_skill_names = HashSet::new();
    let mut installed_rule_names = HashSet::new();
    if skill_root.join("SKILL.md").is_file() {
        let name = if package_root.join("SKILL.md").exists() {
            source
                .rsplit('/')
                .next()
                .unwrap_or(source)
                .trim_start_matches('@')
                .to_owned()
        } else {
            source.rsplit('/').next().unwrap_or(source).to_owned()
        };
        if skills_declared
            .as_deref()
            .is_none_or(|values| values.iter().any(|value| value == "*" || value == &name))
        {
            let destination = curated_skills.join(&name);
            skills += copy_tree(&skill_root, &destination, "overwrite")?;
            installed_skill_names.insert(name);
        }
    } else if skill_root.is_dir() {
        let selected = skills_declared
            .as_deref()
            .filter(|values| !values.iter().any(|value| value == "*"));
        if let Some(selected) = selected {
            let available = direct_dirs(&skill_root)
                .into_iter()
                .filter_map(|path| {
                    path.file_name()
                        .and_then(|value| value.to_str())
                        .map(ToOwned::to_owned)
                })
                .collect::<HashSet<_>>();
            if let Some(unknown) = selected.iter().find(|name| !available.contains(*name)) {
                return Err(anyhow!(
                    "Unknown skill '{unknown}' in npm package '{source}'"
                ));
            }
        }
        let (count, names) = copy_source_skills(
            &skill_root,
            curated_skills,
            skills_declared
                .as_deref()
                .filter(|values| !values.iter().any(|value| value == "*")),
            &HashSet::new(),
        )?;
        skills += count;
        installed_skill_names.extend(names);
    }
    let mut rules = 0usize;
    if rules_declared.is_some() {
        let mut rules_root = package_root.clone();
        if let Some(path) = object.get("rulesPath").and_then(Value::as_str) {
            safe_relative_path(path)?;
            rules_root = package_root.join(path);
        } else if rules_root.join("rules").is_dir() {
            rules_root = rules_root.join("rules");
        } else if rules_root.join(".carabiner/rules").is_dir() {
            rules_root = rules_root.join(".carabiner/rules");
        }
        let (count, names) = copy_selected_rules_excluding(
            &rules_root,
            curated_rules,
            rules_declared.as_deref(),
            &HashSet::new(),
        )?;
        rules += count;
        installed_rule_names.extend(names);
    }
    let mut skill_entries = Map::new();
    for directory in direct_dirs(curated_skills) {
        let main = directory.join("SKILL.md");
        if !main.is_file() {
            continue;
        }
        let name = directory
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        if installed_skill_names.contains(name) {
            skill_entries.insert(name.to_owned(), source_lock_skill_entry(&directory)?);
        }
    }
    let mut lock = Map::from_iter([
        ("registry".into(), Value::String(registry.to_owned())),
        ("resolvedVersion".into(), Value::String(resolved_version)),
        (
            "resolvedAt".into(),
            Value::String(chrono::Utc::now().to_rfc3339()),
        ),
        ("skills".into(), Value::Object(skill_entries)),
    ]);
    if let Some(selected) = rules_declared.as_deref() {
        let mut rules_lock = source_lock_rules(curated_rules, Some(selected))?;
        rules_lock.retain(|name, _| installed_rule_names.contains(name));
        lock.insert("rules".into(), Value::Object(rules_lock));
        lock.insert(
            "ruleSelection".into(),
            Value::Array(selected.iter().cloned().map(Value::String).collect()),
        );
        if let Some(rules_path) = object.get("rulesPath").and_then(Value::as_str) {
            lock.insert("rulesPath".into(), Value::String(rules_path.to_owned()));
        }
    }
    lock.insert(
        "requestedVersion".into(),
        Value::String(requested_version.unwrap_or("latest").to_owned()),
    );
    if !integrity.is_empty() {
        lock.insert("integrity".into(), Value::String(integrity.to_owned()));
    }
    let _ = fs::remove_file(&tarball);
    let _ = fs::remove_dir_all(&staging);
    Ok((skills, rules, Value::Object(lock)))
}

fn install_carabiner_sources(options: &InstallCli) -> Result<Value> {
    let project_root = std::env::current_dir()?;
    let config_options = ConfigOptions {
        config_path: options.config.clone(),
        verbose: options.verbose.then_some(true),
        silent: options.silent.then_some(true),
        ..ConfigOptions::default()
    };
    let config = Config::resolve(&config_options)?;
    let sources = config.sources().to_vec();
    if project_root.join("apm.yml").is_file() && !sources.is_empty() {
        return Err(anyhow!(
            "Both apm.yml and carabiner.jsonc `sources` are defined. Pass --mode apm or --mode carabiner to disambiguate."
        ));
    }
    let curated_skills = project_root.join(".carabiner/skills/.curated");
    let curated_rules = project_root.join(".carabiner/rules/.curated");
    let lock_path = project_root.join("carabiner.lock");
    let npm_lock_path = project_root.join("carabiner-npm.lock.json");
    for path in [&curated_skills, &curated_rules, &lock_path, &npm_lock_path] {
        assert_no_symlink_path(&project_root, path)?;
    }
    if sources.is_empty() {
        if !options.silent && human_output() {
            if project_root.join("apm.yml").is_file() {
                println!("No sources defined in carabiner.jsonc, but apm.yml is present. Did you mean --mode apm?");
            } else {
                println!("No sources defined in configuration. Removing stale source artifacts.");
            }
        }
        if !options.frozen {
            if curated_skills.is_dir() {
                fs::remove_dir_all(&curated_skills)?;
            }
            if curated_rules.is_dir() {
                fs::remove_dir_all(&curated_rules)?;
            }
            write_sources_lock(&lock_path, &Map::new())?;
            write_npm_lock(&npm_lock_path, &Map::new())?;
        }
        return Ok(
            json!({"sourcesProcessed": 0, "skillsFetched": 0, "rulesFetched": 0, "failedSourceCount": 0}),
        );
    }
    let mut lock_sources = read_sources_lock(&lock_path);
    let mut npm_lock_sources = read_npm_lock(&npm_lock_path);
    let declared_keys = sources
        .iter()
        .filter_map(Value::as_object)
        .filter_map(|entry| entry.get("source").and_then(Value::as_str))
        .map(normalized_source_key)
        .collect::<HashSet<_>>();
    let declared_npm_keys = sources
        .iter()
        .filter_map(Value::as_object)
        .filter(|entry| entry.get("transport").and_then(Value::as_str) == Some("npm"))
        .filter_map(|entry| entry.get("source").and_then(Value::as_str))
        .map(|source| source.trim().to_owned())
        .collect::<HashSet<_>>();
    if !options.frozen {
        npm_lock_sources.retain(|key, _| declared_npm_keys.contains(key));
    }
    if !options.frozen {
        lock_sources.retain(|key, _| declared_keys.contains(&normalized_source_key(key)));
    }
    let mut owned_skill_names = HashSet::new();
    for directory in direct_dirs(&project_root.join(".carabiner/skills")) {
        if directory != curated_skills {
            if let Some(name) = directory.file_name().and_then(|value| value.to_str()) {
                owned_skill_names.insert(name.to_owned());
            }
        }
    }
    let mut owned_rule_names = HashSet::new();
    for path in walk_files(&project_root.join(".carabiner/rules")) {
        let relative = relative_slash(&project_root.join(".carabiner/rules"), &path);
        if relative.split('/').any(|part| part == ".curated") {
            continue;
        }
        if path.extension().and_then(|value| value.to_str()) == Some("md") {
            owned_rule_names.insert(relative.strip_suffix(".md").unwrap_or(&relative).to_owned());
        }
    }
    let mut skill_count = 0usize;
    let mut rule_count = 0usize;
    let mut failed = 0usize;
    for source_value in &sources {
        let Some(object) = source_value.as_object() else {
            failed += 1;
            continue;
        };
        let Some(source) = object.get("source").and_then(Value::as_str) else {
            failed += 1;
            continue;
        };
        if object.get("transport").and_then(Value::as_str) == Some("npm") {
            let key = source.trim().to_owned();
            let lock_entry = npm_lock_sources.get(&key).cloned();
            let lock_complete = lock_entry.as_ref().is_some_and(|entry| {
                let Some(entry) = entry.as_object() else {
                    return false;
                };
                let skills_ok = entry
                    .get("skills")
                    .and_then(Value::as_object)
                    .map(|skills| skills.keys().all(|name| curated_skills.join(name).is_dir()))
                    .unwrap_or(false);
                let rules_ok = entry
                    .get("rules")
                    .and_then(Value::as_object)
                    .map(|rules| {
                        rules
                            .keys()
                            .all(|name| curated_rules.join(format!("{name}.md")).is_file())
                    })
                    .unwrap_or(true);
                skills_ok && rules_ok
            });
            if lock_complete && !options.update {
                if let Some(entry) = lock_entry.as_ref() {
                    owned_skill_names.extend(lock_entry_names(entry, "skills"));
                    owned_rule_names.extend(lock_entry_names(entry, "rules"));
                }
                continue;
            }
            if options.frozen && lock_entry.is_none() {
                return Err(anyhow!(
                    "Frozen install failed: npm lockfile is missing entries for: {source}. Run 'carabiner install' to update the lockfile."
                ));
            }
            match install_npm_source(
                object,
                source,
                &project_root,
                &curated_skills,
                &curated_rules,
                options,
            ) {
                Ok((skills, rules, entry)) => {
                    skill_count += skills;
                    rule_count += rules;
                    owned_skill_names.extend(lock_entry_names(&entry, "skills"));
                    owned_rule_names.extend(lock_entry_names(&entry, "rules"));
                    npm_lock_sources.insert(key, entry);
                }
                Err(error) => {
                    failed += 1;
                    if !options.silent && human_output() {
                        eprintln!("Failed to install source \"{source}\": {error}");
                    }
                }
            }
            continue;
        }
        let key = normalized_source_key(source);
        let lock_entry = lock_sources.get(&key).cloned();
        let skills_declared = source_string_list(object, "skills");
        let rules_declared = source_string_list(object, "rules");
        let selected_skills = if skills_declared.is_some() {
            skills_declared.clone()
        } else if rules_declared.is_some() {
            None
        } else {
            Some(vec!["*".into()])
        };
        let requested_ref = object.get("ref").and_then(Value::as_str);
        let requested_ref = requested_ref.or_else(|| {
            lock_entry
                .as_ref()
                .and_then(Value::as_object)
                .and_then(|entry| entry.get("requestedRef"))
                .and_then(Value::as_str)
        });
        let lock_complete = lock_entry.as_ref().is_some_and(|entry| {
            let Some(entry) = entry.as_object() else {
                return false;
            };
            let skills_ok = entry
                .get("skills")
                .and_then(Value::as_object)
                .map(|skills| skills.keys().all(|name| curated_skills.join(name).is_dir()))
                .unwrap_or(false);
            let rules_ok = entry
                .get("rules")
                .and_then(Value::as_object)
                .map(|rules| {
                    rules
                        .keys()
                        .all(|name| curated_rules.join(format!("{name}.md")).is_file())
                })
                .unwrap_or(true);
            skills_ok && rules_ok
        });
        if lock_complete && !options.update {
            if let Some(entry) = lock_entry.as_ref() {
                owned_skill_names.extend(lock_entry_names(entry, "skills"));
                owned_rule_names.extend(lock_entry_names(entry, "rules"));
            }
            continue;
        }
        if options.frozen && lock_entry.is_none() {
            return Err(anyhow!(
                "Frozen install failed: lockfile is missing entries for: {source}. Run 'carabiner install' to update the lockfile."
            ));
        }
        let staging = std::env::temp_dir().join(format!(
            "carabiner-install-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        if let Err(error) = (|| -> Result<()> {
            fs::create_dir_all(&staging)?;
            let mut fetched_skill_names = HashSet::new();
            let mut fetched_rule_names = HashSet::new();
            if selected_skills.is_some() {
                let selected = selected_skills
                    .as_deref()
                    .filter(|values| !values.iter().any(|value| value == "*"));
                let _ = fetch_source_internal(
                    source,
                    "carabiner",
                    &["skills".into()],
                    selected,
                    requested_ref,
                    object.get("path").and_then(Value::as_str),
                    &staging,
                    "overwrite",
                    false,
                )?;
                let (count, names) = copy_source_skills(
                    &staging.join("skills"),
                    &curated_skills,
                    selected,
                    &owned_skill_names,
                )?;
                skill_count += count;
                fetched_skill_names.extend(names);
            }
            if let Some(selected) = rules_declared.as_deref() {
                let rules_path = object
                    .get("rulesPath")
                    .and_then(Value::as_str)
                    .or_else(|| object.get("path").and_then(Value::as_str));
                let rules_staging = staging.join("rule-source");
                fs::create_dir_all(&rules_staging)?;
                let _ = fetch_source_internal(
                    source,
                    "carabiner",
                    &["rules".into()],
                    None,
                    requested_ref,
                    rules_path,
                    &rules_staging,
                    "overwrite",
                    false,
                )?;
                let (count, names) = copy_selected_rules_excluding(
                    &rules_staging.join("rules"),
                    &curated_rules,
                    Some(selected),
                    &owned_rule_names,
                )?;
                rule_count += count;
                fetched_rule_names.extend(names);
            }
            let entry = restrict_source_lock_entry(
                source_lock_entry(
                    source,
                    requested_ref,
                    selected_skills.as_deref(),
                    rules_declared.as_deref(),
                    object.get("rulesPath").and_then(Value::as_str),
                    &curated_skills,
                    &curated_rules,
                )?,
                &fetched_skill_names,
                &fetched_rule_names,
            );
            owned_skill_names.extend(fetched_skill_names);
            owned_rule_names.extend(fetched_rule_names);
            lock_sources.insert(key.clone(), entry);
            Ok(())
        })() {
            failed += 1;
            if !options.silent && human_output() {
                eprintln!("Failed to install source \"{source}\": {error}");
            }
        }
        let _ = fs::remove_dir_all(&staging);
    }
    if !options.frozen {
        write_sources_lock(&lock_path, &lock_sources)?;
        write_npm_lock(&npm_lock_path, &npm_lock_sources)?;
    }
    if failed > 0 {
        return Err(anyhow!(
            "Failed to install {failed} of {} carabiner source(s).",
            sources.len()
        ));
    }
    if !options.silent && human_output() {
        if skill_count > 0 || rule_count > 0 {
            println!(
                "Installed {skill_count} skill(s) and {rule_count} rule(s) from {} source(s).",
                sources.len()
            );
        } else {
            println!(
                "All source artifacts up to date ({} source(s) checked).",
                sources.len()
            );
        }
    }
    Ok(json!({
        "sourcesProcessed": sources.len(),
        "skillsFetched": skill_count,
        "rulesFetched": rule_count,
        "failedSourceCount": failed,
    }))
}

fn install_command(options: &InstallCli) -> Result<Value> {
    let mode = options.mode.as_deref().unwrap_or("carabiner");
    if !matches!(mode, "carabiner" | "apm" | "gh") {
        return Err(anyhow!(
            "Invalid --mode value '{mode}'. Expected one of: carabiner, apm, gh."
        ));
    }
    match mode {
        "apm" => install_apm_manifest(options),
        "gh" => install_gh_command(options),
        _ => install_carabiner_sources(options),
    }
}

fn gh_relative_install_dir(agent: &str, scope: &str) -> Result<String> {
    if scope == "project" {
        return Ok(if agent == "claude-code" {
            ".claude/skills".into()
        } else {
            ".agents/skills".into()
        });
    }
    if scope != "user" {
        return Err(anyhow!("--mode gh: unknown scope '{scope}'"));
    }
    Ok(match agent {
        "github-copilot" => ".copilot/skills",
        "claude-code" => ".claude/skills",
        "cursor" => ".cursor/skills",
        "codex" => ".agents/skills",
        "gemini" => ".gemini/skills",
        "antigravity" => ".gemini/antigravity/skills",
        _ => return Err(anyhow!("--mode gh: unknown agent '{agent}'")),
    }
    .into())
}

fn inject_gh_skill_metadata(
    content: &str,
    source: &str,
    repository: &str,
    reference: &str,
) -> Result<String> {
    let parsed = parse_frontmatter(content, Path::new("SKILL.md"))?;
    let mut frontmatter = parsed.data;
    frontmatter.insert("source".into(), Value::String(source.into()));
    frontmatter.insert("repository".into(), Value::String(repository.into()));
    frontmatter.insert("ref".into(), Value::String(reference.into()));
    if parsed.has_frontmatter {
        stringify_frontmatter(&parsed.body, &frontmatter)
    } else {
        let yaml = serde_yaml::to_string(&Value::Object(frontmatter))?;
        Ok(format!("---\n{yaml}---\n{content}"))
    }
}

fn copy_gh_skill(
    source: &Path,
    destination: &Path,
    scope_root: &Path,
    source_name: &str,
    repository: &str,
    reference: &str,
) -> Result<(usize, Vec<String>, String)> {
    let mut deployed = Vec::new();
    let mut hash_input = Vec::new();
    for path in walk_files(source) {
        let relative = relative_slash(source, &path);
        safe_relative_path(&relative)?;
        let target = destination.join(&relative);
        let content = if relative == "SKILL.md" {
            inject_gh_skill_metadata(
                &fs::read_to_string(&path)?,
                source_name,
                repository,
                reference,
            )?
            .into_bytes()
        } else {
            fs::read(&path)?
        };
        write_bytes(&target, &content, false)?;
        let relative_to_scope = relative_slash(scope_root, &target);
        safe_relative_path(&relative_to_scope)?;
        deployed.push(relative_to_scope.clone());
        hash_input.extend_from_slice(relative_to_scope.as_bytes());
        hash_input.push(0);
        hash_input.extend_from_slice(&content);
        hash_input.push(0);
    }
    deployed.sort();
    let skill_count = usize::from(source.join("SKILL.md").is_file());
    Ok((
        skill_count,
        deployed,
        sha256_bytes(&hash_input).replacen("sha256-", "sha256:", 1),
    ))
}

fn install_gh_command(options: &InstallCli) -> Result<Value> {
    let project_root = std::env::current_dir()?;
    let config = Config::resolve(&ConfigOptions {
        config_path: options.config.clone(),
        verbose: options.verbose.then_some(true),
        silent: options.silent.then_some(true),
        ..ConfigOptions::default()
    })?;
    let sources = config.sources().to_vec();
    if sources.is_empty() {
        if !options.silent && human_output() {
            println!("No sources defined in configuration. Nothing to install.");
        }
        return Ok(
            json!({"sourcesProcessed": 0, "installedSkillCount": 0, "failedSourceCount": 0}),
        );
    }
    let lock_path = project_root.join("carabiner-gh.lock.yaml");
    assert_no_symlink_path(&project_root, &lock_path)?;
    let existing_lock = if lock_path.is_file() {
        serde_yaml::from_str::<serde_yaml::Value>(&fs::read_to_string(&lock_path)?)
            .unwrap_or(serde_yaml::Value::Null)
    } else {
        serde_yaml::Value::Null
    };
    let existing_installations = existing_lock
        .get("installations")
        .and_then(serde_yaml::Value::as_sequence)
        .cloned()
        .unwrap_or_default();
    if options.frozen && !lock_path.is_file() {
        return Err(anyhow!(
            "Frozen install failed: carabiner-gh.lock.yaml is missing. Run 'carabiner install --mode gh' to create it."
        ));
    }
    let mut installations = Vec::new();
    let mut installed = 0usize;
    let mut failed = 0usize;
    for source_value in &sources {
        let Some(entry) = source_value.as_object() else {
            failed += 1;
            continue;
        };
        let Some(source) = entry.get("source").and_then(Value::as_str) else {
            failed += 1;
            continue;
        };
        if let Some(transport) = entry.get("transport").and_then(Value::as_str) {
            if transport != "github" {
                return Err(anyhow!(
                    "--mode gh: field \"transport\" is not supported (got \"{transport}\" for source \"{source}\"). Drop the field or switch to --mode carabiner."
                ));
            }
        }
        for key in ["path", "rules", "rulesPath"] {
            if entry.contains_key(key) {
                return Err(anyhow!(
                    "--mode gh: field \"{key}\" is not supported for source \"{source}\"."
                ));
            }
        }
        let agent = entry
            .get("agent")
            .and_then(Value::as_str)
            .unwrap_or("github-copilot");
        if !matches!(
            agent,
            "github-copilot" | "claude-code" | "cursor" | "codex" | "gemini" | "antigravity"
        ) {
            return Err(anyhow!(
                "--mode gh: unknown agent \"{agent}\" for source \"{source}\". Valid agents: github-copilot, claude-code, cursor, codex, gemini, antigravity."
            ));
        }
        let scope = entry
            .get("scope")
            .and_then(Value::as_str)
            .unwrap_or("project");
        let install_dir = gh_relative_install_dir(agent, scope)?;
        let selected = source_string_list(entry, "skills");
        let requested_ref = entry.get("ref").and_then(Value::as_str);
        let source_info = parse_fetch_remote_source(source);
        let (repository, parsed_ref) = if let Ok((url, parsed_ref, _)) = &source_info {
            let repository = url
                .trim_start_matches("https://github.com/")
                .trim_start_matches("http://github.com/")
                .trim_end_matches(".git")
                .to_owned();
            (repository, parsed_ref.clone())
        } else if Path::new(source).is_dir() {
            (
                format!(
                    "local/{}",
                    Path::new(source)
                        .file_name()
                        .and_then(|v| v.to_str())
                        .unwrap_or("source")
                ),
                None,
            )
        } else {
            return Err(source_info.expect_err("source_info is Err in this branch"));
        };
        let resolved_requested_ref = requested_ref.or(parsed_ref.as_deref());
        let matching = |skill: Option<&str>| {
            existing_installations.iter().find(|installation| {
                let Some(object) = installation.as_mapping() else {
                    return false;
                };
                let get = |key: &str| {
                    object
                        .get(serde_yaml::Value::String(key.into()))
                        .and_then(serde_yaml::Value::as_str)
                };
                get("source").is_some_and(|value| value.eq_ignore_ascii_case(source))
                    && get("agent") == Some(agent)
                    && get("scope") == Some(scope)
                    && skill.is_none_or(|wanted| get("skill") == Some(wanted))
            })
        };
        let selected_names = selected
            .as_deref()
            .filter(|values| !values.iter().any(|value| value == "*"));
        let locked_for_source = existing_installations
            .iter()
            .filter(|installation| {
                let Some(object) = installation.as_mapping() else {
                    return false;
                };
                let get = |key: &str| {
                    object
                        .get(serde_yaml::Value::String(key.into()))
                        .and_then(serde_yaml::Value::as_str)
                };
                get("source").is_some_and(|value| value.eq_ignore_ascii_case(source))
                    && get("agent") == Some(agent)
                    && get("scope") == Some(scope)
            })
            .cloned()
            .collect::<Vec<_>>();
        let scope_root = if scope == "user" {
            home_dir()?
        } else {
            project_root.clone()
        };
        let locked_files_exist = |installation: &serde_yaml::Value| {
            installation
                .get("deployed_files")
                .and_then(serde_yaml::Value::as_sequence)
                .map(|files| {
                    files.iter().all(|file| {
                        file.as_str()
                            .map(|file| scope_root.join(file).is_file())
                            .unwrap_or(false)
                    })
                })
                .unwrap_or(false)
        };
        if !options.update
            && !locked_for_source.is_empty()
            && selected_names.is_none_or(|names| {
                names
                    .iter()
                    .all(|name| matching(Some(name)).is_some_and(locked_files_exist))
            })
            && locked_for_source.iter().all(locked_files_exist)
        {
            installations.extend(locked_for_source);
            continue;
        }
        if options.frozen && locked_for_source.is_empty() {
            return Err(anyhow!(
                "Frozen install failed: carabiner-gh.lock.yaml is missing entries for: {source} (agent={agent}, scope={scope}). Run 'carabiner install --mode gh' to update the lockfile."
            ));
        }
        let staging = std::env::temp_dir().join(format!(
            "carabiner-gh-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let result = (|| -> Result<Vec<serde_yaml::Value>> {
            fs::create_dir_all(&staging)?;
            let lock_ref = locked_for_source
                .first()
                .and_then(|installation| installation.get("resolved_commit"))
                .and_then(serde_yaml::Value::as_str);
            let _ = fetch_source_internal(
                source,
                "carabiner",
                &["skills".into()],
                selected_names,
                resolved_requested_ref.or(lock_ref),
                None,
                &staging,
                "overwrite",
                false,
            )?;
            let resolved_commit = git_revision(source, resolved_requested_ref);
            let provenance_ref = resolved_requested_ref.unwrap_or(&resolved_commit);
            let mut output = Vec::new();
            for skill_dir in direct_dirs(&staging.join("skills")) {
                let name = skill_dir
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("")
                    .to_owned();
                if let Some(selected) = selected_names {
                    if !selected.iter().any(|wanted| wanted == &name) {
                        continue;
                    }
                }
                if !skill_dir.join("SKILL.md").is_file() {
                    continue;
                }
                let destination = scope_root.join(&install_dir).join(&name);
                let (skill_count, deployed, content_hash) = copy_gh_skill(
                    &skill_dir,
                    &destination,
                    &scope_root,
                    source,
                    &repository,
                    provenance_ref,
                )?;
                if skill_count == 0 {
                    continue;
                }
                let mut installation = serde_yaml::Mapping::new();
                let put = |key: &str,
                           value: serde_yaml::Value,
                           installation: &mut serde_yaml::Mapping| {
                    installation.insert(serde_yaml::Value::String(key.into()), value);
                };
                put(
                    "source",
                    serde_yaml::Value::String(source.into()),
                    &mut installation,
                );
                let mut coordinates = repository.split('/');
                put(
                    "owner",
                    serde_yaml::Value::String(coordinates.next().unwrap_or("local").into()),
                    &mut installation,
                );
                put(
                    "repo",
                    serde_yaml::Value::String(coordinates.next().unwrap_or("source").into()),
                    &mut installation,
                );
                put(
                    "agent",
                    serde_yaml::Value::String(agent.into()),
                    &mut installation,
                );
                put(
                    "scope",
                    serde_yaml::Value::String(scope.into()),
                    &mut installation,
                );
                put("skill", serde_yaml::Value::String(name), &mut installation);
                if let Some(requested_ref) = resolved_requested_ref {
                    put(
                        "requested_ref",
                        serde_yaml::Value::String(requested_ref.into()),
                        &mut installation,
                    );
                }
                put(
                    "resolved_ref",
                    serde_yaml::Value::String(provenance_ref.into()),
                    &mut installation,
                );
                put(
                    "resolved_commit",
                    serde_yaml::Value::String(resolved_commit.clone()),
                    &mut installation,
                );
                put(
                    "install_dir",
                    serde_yaml::Value::String(install_dir.clone()),
                    &mut installation,
                );
                put(
                    "deployed_files",
                    serde_yaml::Value::Sequence(
                        deployed
                            .into_iter()
                            .map(serde_yaml::Value::String)
                            .collect(),
                    ),
                    &mut installation,
                );
                put(
                    "content_hash",
                    serde_yaml::Value::String(content_hash),
                    &mut installation,
                );
                output.push(serde_yaml::Value::Mapping(installation));
            }
            Ok(output)
        })();
        let _ = fs::remove_dir_all(&staging);
        match result {
            Ok(values) => {
                installed += values.len();
                installations.extend(values);
            }
            Err(error) => {
                failed += 1;
                if !options.silent && human_output() {
                    eprintln!("Failed to install gh source \"{source}\": {error}");
                }
                installations.extend(locked_for_source);
            }
        }
    }
    if !options.frozen {
        let mut lock = serde_yaml::Mapping::new();
        lock.insert(
            serde_yaml::Value::String("lockfile_version".into()),
            serde_yaml::Value::String("1".into()),
        );
        lock.insert(
            serde_yaml::Value::String("generated_at".into()),
            serde_yaml::Value::String(chrono::Utc::now().to_rfc3339()),
        );
        lock.insert(
            serde_yaml::Value::String("installations".into()),
            serde_yaml::Value::Sequence(installations),
        );
        write_text(
            &lock_path,
            &serde_yaml::to_string(&serde_yaml::Value::Mapping(lock))?,
            false,
        )?;
    }
    if failed > 0 {
        return Err(anyhow!(
            "Failed to install {failed} of {} gh source(s).",
            sources.len()
        ));
    }
    if !options.silent && human_output() {
        if installed > 0 {
            println!(
                "Installed {installed} skill(s) from {} gh source(s).",
                sources.len()
            );
        } else {
            println!("All gh sources up to date ({} checked).", sources.len());
        }
    }
    Ok(json!({
        "sourcesProcessed": sources.len(),
        "installedSkillCount": installed,
        "failedSourceCount": failed,
    }))
}

type ApmDependency = (String, Option<String>, Option<String>);

fn apm_dependencies(value: &serde_yaml::Value) -> Result<Vec<ApmDependency>> {
    let entries = value
        .get("dependencies")
        .and_then(|value| value.get("apm"))
        .and_then(serde_yaml::Value::as_sequence)
        .cloned()
        .unwrap_or_default();
    let mut dependencies = Vec::new();
    for (index, entry) in entries.into_iter().enumerate() {
        match entry {
            serde_yaml::Value::String(raw) => {
                let (source, reference) = raw
                    .split_once('#')
                    .map(|(source, reference)| (source.to_owned(), Some(reference.to_owned())))
                    .unwrap_or((raw, None));
                if source.trim().is_empty() {
                    return Err(anyhow!(
                        "apm.yml dependency #{} must not be empty",
                        index + 1
                    ));
                }
                dependencies.push((source, reference, None));
            }
            serde_yaml::Value::Mapping(object) => {
                let source = object
                    .get(serde_yaml::Value::String("git".into()))
                    .or_else(|| object.get(serde_yaml::Value::String("source".into())))
                    .and_then(serde_yaml::Value::as_str)
                    .map(ToOwned::to_owned)
                    .ok_or_else(|| {
                        anyhow!("apm.yml dependency #{} requires a git field", index + 1)
                    })?;
                let reference = object
                    .get(serde_yaml::Value::String("ref".into()))
                    .and_then(serde_yaml::Value::as_str)
                    .map(ToOwned::to_owned);
                let path = object
                    .get(serde_yaml::Value::String("path".into()))
                    .and_then(serde_yaml::Value::as_str)
                    .map(ToOwned::to_owned);
                if path.as_deref().is_some_and(|path| {
                    path.is_empty()
                        || Path::new(path).is_absolute()
                        || path.split(&['/', '\\'][..]).any(|part| part == "..")
                }) {
                    return Err(anyhow!(
                        "apm.yml dependency #{} has an unsafe path",
                        index + 1
                    ));
                }
                dependencies.push((source, reference, path));
            }
            _ => {
                return Err(anyhow!(
                    "apm.yml dependency #{} must be a string or object",
                    index + 1
                ))
            }
        }
    }
    Ok(dependencies)
}

fn clone_apm_source(
    source: &str,
    requested_ref: Option<&str>,
) -> Result<(PathBuf, Option<PathBuf>, String)> {
    let local = PathBuf::from(source);
    if local.is_dir() {
        let root = if local.is_absolute() {
            local
        } else {
            std::env::current_dir()?.join(local)
        };
        return Ok((root, None, source.to_owned()));
    }
    let (url, inferred_ref, _) = parse_fetch_remote_source(source)?;
    let requested_ref = requested_ref.or(inferred_ref.as_deref());
    if requested_ref.is_some_and(|value| value.starts_with('-')) {
        return Err(anyhow!("apm dependency ref must not start with '-'"));
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temp = std::env::temp_dir().join(format!("carabiner-apm-{}-{stamp}", std::process::id()));
    let mut command = Command::new("git");
    command.arg("clone").arg("--depth").arg("1");
    if let Some(reference) = requested_ref {
        command.arg("--branch").arg(reference);
    }
    command.arg(&url).arg(&temp);
    let status = command.status().context("failed to start git")?;
    if !status.success() {
        let _ = fs::remove_dir_all(&temp);
        return Err(anyhow!("git clone failed for APM dependency {source}"));
    }
    Ok((temp.clone(), Some(temp), url))
}

fn copy_apm_primitive(
    source: &Path,
    destination: &Path,
    project_root: &Path,
) -> Result<(usize, Vec<String>, Vec<u8>)> {
    let mut count = 0;
    let mut paths = Vec::new();
    let mut hash_input = Vec::new();
    if !source.is_dir() {
        return Ok((0, paths, hash_input));
    }
    for path in walk_files(source) {
        let relative = path.strip_prefix(source).unwrap_or(&path);
        let relative_string = relative.to_string_lossy().replace('\\', "/");
        let target = destination.join(relative);
        assert_no_symlink_path(destination, &target)?;
        let bytes = fs::read(&path)?;
        if write_bytes(&target, &bytes, false)? {
            count += 1;
        }
        let deploy_relative = relative_slash(project_root, &target);
        safe_relative_path(&deploy_relative)?;
        paths.push(deploy_relative.clone());
        hash_input.extend_from_slice(deploy_relative.as_bytes());
        hash_input.push(0);
        hash_input.extend_from_slice(&bytes);
        hash_input.push(0);
        let _ = relative_string;
    }
    Ok((count, paths, hash_input))
}

fn install_apm_manifest(options: &InstallCli) -> Result<Value> {
    let project_root = std::env::current_dir()?;
    let path = project_root.join("apm.yml");
    if !path.is_file() {
        return Err(anyhow!(
            "--mode apm requires an apm.yml at the project root. Create one or drop --mode apm to fall back to carabiner mode."
        ));
    }
    let content = fs::read_to_string(&path)?;
    let yaml: serde_yaml::Value = serde_yaml::from_str(&content).context("invalid apm.yml")?;
    let dependencies = apm_dependencies(&yaml)?;
    if dependencies.is_empty() {
        if !options.silent && human_output() {
            println!("apm.yml has no dependencies.apm entries. Nothing to install.");
        }
        return Ok(
            json!({"dependenciesProcessed": 0, "deployedFileCount": 0, "failedDependencyCount": 0}),
        );
    }
    let lock_path = project_root.join("carabiner-apm.lock.yaml");
    assert_no_symlink_path(&project_root, &lock_path)?;
    let existing_lock = if lock_path.is_file() {
        serde_yaml::from_str::<serde_yaml::Value>(&fs::read_to_string(&lock_path)?)
            .unwrap_or(serde_yaml::Value::Null)
    } else {
        serde_yaml::Value::Null
    };
    let existing_entries = existing_lock
        .get("dependencies")
        .and_then(serde_yaml::Value::as_sequence)
        .cloned()
        .unwrap_or_default();
    if options.frozen {
        if !lock_path.is_file() {
            return Err(anyhow!(
                "Frozen install failed: carabiner-apm.lock.yaml is missing. Run 'carabiner install --mode apm' to create it."
            ));
        }
        for (source, _, _) in &dependencies {
            let canonical = if let Ok((_, _, url)) = clone_apm_source(source, None) {
                url
            } else {
                source.clone()
            };
            if !existing_entries.iter().any(|entry| {
                entry
                    .get("repo_url")
                    .and_then(serde_yaml::Value::as_str)
                    .map(|value| {
                        value == canonical
                            || value.trim_end_matches(".git") == canonical.trim_end_matches(".git")
                    })
                    .unwrap_or(false)
            }) {
                return Err(anyhow!(
                "Frozen install failed: carabiner-apm.lock.yaml is missing entries for: {source}. Run 'carabiner install --mode apm' to update the lockfile."
                ));
            }
        }
    }
    let mut lock_entries = Vec::new();
    let mut deployed_total = 0usize;
    let mut failed = 0usize;
    for (source, requested_ref, dependency_path) in &dependencies {
        let result = (|| -> Result<(usize, serde_yaml::Value)> {
            let (root, temporary, canonical_url) =
                clone_apm_source(source, requested_ref.as_deref())?;
            let _temporary_cleanup = TempDirGuard::new(temporary.clone());
            let resolved_commit =
                git_revision(root.to_string_lossy().as_ref(), requested_ref.as_deref());
            let root = if let Some(path) = dependency_path {
                safe_relative_path(path)?;
                root.join(path)
            } else {
                root
            };
            let mut deployed_files = Vec::new();
            let mut hash_input = Vec::new();
            let (instruction_count, mut instruction_paths, instruction_hash) = copy_apm_primitive(
                &root.join(".apm/instructions"),
                &project_root.join(".github/instructions"),
                &project_root,
            )?;
            hash_input.extend(instruction_hash);
            deployed_files.append(&mut instruction_paths);
            let (skill_count, mut skill_paths, skill_hash) = copy_apm_primitive(
                &root.join(".apm/skills"),
                &project_root.join(".github/skills"),
                &project_root,
            )?;
            hash_input.extend(skill_hash);
            deployed_files.append(&mut skill_paths);
            deployed_files.sort();
            let mut entry = serde_yaml::Mapping::new();
            entry.insert(
                serde_yaml::Value::String("repo_url".into()),
                serde_yaml::Value::String(canonical_url),
            );
            entry.insert(
                serde_yaml::Value::String("resolved_commit".into()),
                serde_yaml::Value::String(resolved_commit),
            );
            entry.insert(
                serde_yaml::Value::String("resolved_ref".into()),
                serde_yaml::Value::String(requested_ref.clone().unwrap_or_else(|| "HEAD".into())),
            );
            entry.insert(
                serde_yaml::Value::String("depth".into()),
                serde_yaml::Value::Number(1.into()),
            );
            entry.insert(
                serde_yaml::Value::String("package_type".into()),
                serde_yaml::Value::String("apm_package".into()),
            );
            entry.insert(
                serde_yaml::Value::String("content_hash".into()),
                serde_yaml::Value::String(
                    sha256_bytes(&hash_input).replacen("sha256-", "sha256:", 1),
                ),
            );
            entry.insert(
                serde_yaml::Value::String("deployed_files".into()),
                serde_yaml::Value::Sequence(
                    deployed_files
                        .iter()
                        .cloned()
                        .map(serde_yaml::Value::String)
                        .collect(),
                ),
            );
            if let Some(path) = dependency_path {
                entry.insert(
                    serde_yaml::Value::String("virtual_path".into()),
                    serde_yaml::Value::String(path.clone()),
                );
            }
            if let Some(temporary) = temporary {
                let _ = fs::remove_dir_all(temporary);
            }
            Ok((
                instruction_count + skill_count,
                serde_yaml::Value::Mapping(entry),
            ))
        })();
        match result {
            Ok((count, entry)) => {
                deployed_total += count;
                lock_entries.push(entry);
            }
            Err(error) => {
                failed += 1;
                if !options.silent && human_output() {
                    eprintln!("Failed to install apm dependency \"{source}\": {error}");
                }
                if let Some(previous) = existing_entries.iter().find(|entry| {
                    entry
                        .get("repo_url")
                        .and_then(serde_yaml::Value::as_str)
                        .is_some_and(|url| url == source)
                }) {
                    lock_entries.push(previous.clone());
                }
            }
        }
    }
    if !options.frozen {
        let mut lock = serde_yaml::Mapping::new();
        lock.insert(
            serde_yaml::Value::String("apm_version".into()),
            serde_yaml::Value::String("carabiner-compat/0.1".into()),
        );
        lock.insert(
            serde_yaml::Value::String("generated_at".into()),
            serde_yaml::Value::String(chrono::Utc::now().to_rfc3339()),
        );
        lock.insert(
            serde_yaml::Value::String("dependencies".into()),
            serde_yaml::Value::Sequence(lock_entries),
        );
        write_text(
            &lock_path,
            &serde_yaml::to_string(&serde_yaml::Value::Mapping(lock))?,
            false,
        )?;
    }
    if failed > 0 {
        return Err(anyhow!(
            "Failed to install {failed} of {} apm dependency(ies).",
            dependencies.len()
        ));
    }
    if !options.silent && human_output() {
        if deployed_total > 0 {
            println!(
                "Installed {deployed_total} file(s) from {} apm dependency(ies).",
                dependencies.len()
            );
        } else {
            println!(
                "All apm dependencies up to date ({} checked).",
                dependencies.len()
            );
        }
    }
    Ok(json!({
        "dependenciesProcessed": dependencies.len(),
        "deployedFileCount": deployed_total,
        "failedDependencyCount": failed,
    }))
}

fn doctor_invalid_value(
    file: &str,
    path: &str,
    message: impl Into<String>,
    diagnostics: &mut Vec<Value>,
) {
    diagnostics.push(json!({
        "severity": "error",
        "code": "config/invalid-value",
        "file": file,
        "message": format!("Invalid value at '{path}': {}", message.into()),
    }));
}

fn valid_environment_name(value: &str) -> bool {
    let mut characters = value.chars();
    matches!(characters.next(), Some(first) if first.is_ascii_alphabetic() || first == '_')
        && characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

fn doctor_command(options: &DoctorCli) -> Result<Value> {
    let cwd = std::env::current_dir()?;
    let raw_config = options.config.as_deref().unwrap_or("carabiner.jsonc");
    let candidate = PathBuf::from(raw_config);
    let path = if candidate.is_absolute() {
        candidate
    } else {
        if candidate
            .components()
            .any(|component| component == std::path::Component::ParentDir)
        {
            return Err(anyhow!(
                "Path traversal detected in config path '{raw_config}'"
            ));
        }
        cwd.join(candidate)
    };
    if !path.starts_with(&cwd) {
        return Err(anyhow!(
            "Path traversal detected in config path '{raw_config}'"
        ));
    }
    let display_path = |path: &Path| relative_slash(&cwd, path).replace('\\', "/");
    let local_path = path.parent().unwrap_or(&cwd).join("carabiner.local.jsonc");
    let mut diagnostics = Vec::new();
    let mut parsed_files: Vec<(PathBuf, Map<String, Value>)> = Vec::new();
    for file_path in [&path, &local_path] {
        if !file_path.is_file() {
            continue;
        }
        let file = display_path(file_path);
        let content = fs::read_to_string(file_path)?;
        if content.trim().is_empty() {
            diagnostics.push(json!({
                "severity": "warning",
                "code": "config/empty-file",
                "file": file,
                "message": "Configuration file is empty; carabiner will run with built-in defaults."
            }));
            continue;
        }
        let value = match parse_jsonc(&content) {
            Ok(value) => value,
            Err(error) => {
                let mut item = json!({
                    "severity": "error",
                    "code": "config/parse-error",
                    "file": file,
                    "message": format!("JSONC parse error: {}.", format_error(&error))
                });
                if let Some(parse_error) = error
                    .chain()
                    .find_map(|cause| cause.downcast_ref::<serde_json::Error>())
                {
                    if let Some(object) = item.as_object_mut() {
                        object.insert(
                            "line".into(),
                            Value::Number((parse_error.line() as u64).into()),
                        );
                        object.insert(
                            "column".into(),
                            Value::Number((parse_error.column() as u64).into()),
                        );
                    }
                }
                diagnostics.push(item);
                continue;
            }
        };
        let Some(object) = value.as_object().cloned() else {
            diagnostics.push(json!({
                "severity": "error",
                "code": "config/not-an-object",
                "file": file,
                "message": format!("Configuration file must contain a JSON object, found {}.", value)
            }));
            continue;
        };
        let known = [
            "$schema",
            "targets",
            "features",
            "outputRoots",
            "verbose",
            "silent",
            "delete",
            "global",
            "simulateCommands",
            "simulateSubagents",
            "simulateSkills",
            "flattenedCommandNaming",
            "gitignoreTargetsOnly",
            "gitignoreDestination",
            "dryRun",
            "check",
            "inputRoot",
            "inputRoots",
            "sources",
        ];
        for key in object.keys().filter(|key| !known.contains(&key.as_str())) {
            let hint = nearest(key, &known);
            let mut item = json!({
                "severity": "error",
                "code": "config/unknown-key",
                "file": file,
                "message": format!("Unknown key '{key}'. It is silently ignored by 'carabiner generate'.")
            });
            if !hint.is_empty() {
                item["hint"] = Value::String(hint);
            } else {
                item["hint"] = Value::String(format!("Known keys: {}.", known.join(", ")));
            }
            diagnostics.push(item);
        }
        match object.get("$schema") {
            None => diagnostics.push(json!({
                "severity": "info",
                "code": "config/missing-schema",
                "file": file,
                "message": "No '$schema' property; editors cannot offer completion and validation.",
                "hint": "Add \"$schema\": \"https://github.com/findyourexit/carabiner/releases/latest/download/config-schema.json\"."
            })),
            Some(Value::String(schema)) if schema != "https://github.com/findyourexit/carabiner/releases/latest/download/config-schema.json" => diagnostics.push(json!({
                "severity": "warning",
                "code": "config/outdated-schema",
                "file": file,
                "message": "'$schema' does not point at the current Carabiner config schema.",
                "hint": "Update it to \"https://github.com/findyourexit/carabiner/releases/latest/download/config-schema.json\"."
            })),
            Some(value) if !value.is_string() => diagnostics.push(json!({
                "severity": "error",
                "code": "config/invalid-value",
                "file": file,
                "message": "Invalid value at '$schema': expected a string."
            })),
            _ => {}
        }
        check_doctor_targets(object.get("targets"), &file, &mut diagnostics);
        check_doctor_features(object.get("features"), &file, &mut diagnostics);
        if object.get("targets").and_then(Value::as_object).is_some()
            && object.get("features").is_some()
        {
            diagnostics.push(json!({
                "severity": "error",
                "code": "config/targets-features-conflict",
                "file": file,
                "message": "When 'targets' is in object form, 'features' must be omitted.",
                "hint": "Declare per-target features inside the 'targets' object instead."
            }));
        }
        if object.contains_key("inputRoot") && object.contains_key("inputRoots") {
            diagnostics.push(json!({
                "severity": "error",
                "code": "config/input-roots-conflict",
                "file": file,
                "message": "'inputRoot' and 'inputRoots' cannot be combined in the same config file.",
                "hint": "Remove 'inputRoot' and keep 'inputRoots', or keep only the singular field."
            }));
        }
        for key in [
            "verbose",
            "silent",
            "delete",
            "global",
            "simulateCommands",
            "simulateSubagents",
            "simulateSkills",
            "gitignoreTargetsOnly",
            "dryRun",
            "check",
        ] {
            if object.get(key).is_some_and(|value| !value.is_boolean()) {
                diagnostics.push(json!({
                    "severity": "error",
                    "code": "config/invalid-value",
                    "file": file,
                    "message": format!("Invalid value at '{key}': expected a boolean.")
                }));
            }
        }
        if let Some(value) = object.get("flattenedCommandNaming") {
            if !matches!(value.as_str(), Some("basename" | "path")) {
                doctor_invalid_value(
                    &file,
                    "flattenedCommandNaming",
                    "expected one of basename or path",
                    &mut diagnostics,
                );
            }
        }
        if let Some(value) = object.get("gitignoreDestination") {
            if !matches!(value.as_str(), Some("gitignore" | "gitattributes")) {
                doctor_invalid_value(
                    &file,
                    "gitignoreDestination",
                    "expected one of gitignore or gitattributes",
                    &mut diagnostics,
                );
            }
        }
        if let Some(value) = object.get("inputRoot") {
            if !value.is_string() {
                doctor_invalid_value(
                    &file,
                    "inputRoot",
                    format!("expected a string, found {value}"),
                    &mut diagnostics,
                );
            }
        }
        if let Some(value) = object.get("outputRoots") {
            match value {
                Value::Array(values) => {
                    for (index, item) in values.iter().enumerate() {
                        if !item.is_string() {
                            doctor_invalid_value(
                                &file,
                                &format!("outputRoots.{index}"),
                                "expected a string",
                                &mut diagnostics,
                            );
                        }
                    }
                }
                Value::Object(values) => {
                    for (key, item) in values {
                        let valid = item.as_str().is_some()
                            || item
                                .as_array()
                                .is_some_and(|items| items.iter().all(Value::is_string));
                        if !valid {
                            doctor_invalid_value(
                                &file,
                                &format!("outputRoots.{key}"),
                                "expected a string or an array of strings",
                                &mut diagnostics,
                            );
                        }
                    }
                }
                _ => doctor_invalid_value(
                    &file,
                    "outputRoots",
                    "expected an array or object",
                    &mut diagnostics,
                ),
            }
        }

        if let Some(value) = object.get("inputRoots") {
            if let Some(values) = value.as_array() {
                if values.is_empty() {
                    doctor_invalid_value(
                        &file,
                        "inputRoots",
                        "inputRoots must be non-empty",
                        &mut diagnostics,
                    );
                }
                for (index, item) in values.iter().enumerate() {
                    if !item.is_string() {
                        doctor_invalid_value(
                            &file,
                            &format!("inputRoots.{index}"),
                            "expected a string",
                            &mut diagnostics,
                        );
                    }
                }
            } else {
                doctor_invalid_value(&file, "inputRoots", "expected an array", &mut diagnostics);
            }
        }
        if let Some(sources) = object.get("sources") {
            if let Some(values) = sources.as_array() {
                for (index, source) in values.iter().enumerate() {
                    let source_path = format!("sources.{index}");
                    let Some(source) = source.as_object() else {
                        doctor_invalid_value(
                            &file,
                            &source_path,
                            format!("expected an object, found {source}."),
                            &mut diagnostics,
                        );
                        continue;
                    };
                    match source.get("source").and_then(Value::as_str) {
                        Some(value) if !value.is_empty() => {}
                        Some(_) => doctor_invalid_value(
                            &file,
                            &format!("{source_path}.source"),
                            "source must be a non-empty string",
                            &mut diagnostics,
                        ),
                        None => doctor_invalid_value(
                            &file,
                            &format!("{source_path}.source"),
                            "source must be a non-empty string",
                            &mut diagnostics,
                        ),
                    }
                    for key in ["skills", "rules"] {
                        if let Some(value) = source.get(key) {
                            let Some(items) = value.as_array() else {
                                doctor_invalid_value(
                                    &file,
                                    &format!("{source_path}.{key}"),
                                    "expected an array",
                                    &mut diagnostics,
                                );
                                continue;
                            };
                            for (item_index, item) in items.iter().enumerate() {
                                if !item.is_string() {
                                    doctor_invalid_value(
                                        &file,
                                        &format!("{source_path}.{key}.{item_index}"),
                                        "expected a string",
                                        &mut diagnostics,
                                    );
                                }
                            }
                        }
                    }
                    if let Some(value) = source.get("transport") {
                        if !matches!(value.as_str(), Some("github" | "git" | "npm")) {
                            doctor_invalid_value(
                                &file,
                                &format!("{source_path}.transport"),
                                "expected one of github, git, or npm",
                                &mut diagnostics,
                            );
                        }
                    }
                    if let Some(value) = source.get("ref") {
                        if let Some(value) = value.as_str() {
                            if value.starts_with('-') {
                                doctor_invalid_value(
                                    &file,
                                    &format!("{source_path}.ref"),
                                    "ref must not start with '-'",
                                    &mut diagnostics,
                                );
                            } else if value.chars().any(char::is_control) {
                                doctor_invalid_value(
                                    &file,
                                    &format!("{source_path}.ref"),
                                    "ref must not contain control characters",
                                    &mut diagnostics,
                                );
                            }
                        } else {
                            doctor_invalid_value(
                                &file,
                                &format!("{source_path}.ref"),
                                "expected a string",
                                &mut diagnostics,
                            );
                        }
                    }
                    for key in ["path", "rulesPath"] {
                        if let Some(value) = source.get(key) {
                            let Some(value) = value.as_str() else {
                                doctor_invalid_value(
                                    &file,
                                    &format!("{source_path}.{key}"),
                                    "expected a string",
                                    &mut diagnostics,
                                );
                                continue;
                            };
                            if value.contains("..") {
                                doctor_invalid_value(
                                    &file,
                                    &format!("{source_path}.{key}"),
                                    format!("{key} must not contain '..'"),
                                    &mut diagnostics,
                                );
                            } else if Path::new(value).is_absolute() {
                                doctor_invalid_value(
                                    &file,
                                    &format!("{source_path}.{key}"),
                                    format!("{key} must not be absolute"),
                                    &mut diagnostics,
                                );
                            } else if value.chars().any(char::is_control) {
                                doctor_invalid_value(
                                    &file,
                                    &format!("{source_path}.{key}"),
                                    format!("{key} must not contain control characters"),
                                    &mut diagnostics,
                                );
                            }
                        }
                    }
                    if let Some(value) = source.get("registry") {
                        let valid = value.as_str().is_some_and(|value| {
                            (value.starts_with("https://") || value.starts_with("http://"))
                                && !value.chars().any(char::is_control)
                        });
                        if !valid {
                            doctor_invalid_value(
                                &file,
                                &format!("{source_path}.registry"),
                                "registry must be an http(s) URL",
                                &mut diagnostics,
                            );
                        }
                    }
                    if let Some(value) = source.get("tokenEnv") {
                        if let Some(value) = value.as_str() {
                            if !valid_environment_name(value) {
                                doctor_invalid_value(
                                    &file,
                                    &format!("{source_path}.tokenEnv"),
                                    "tokenEnv must be a valid environment variable name",
                                    &mut diagnostics,
                                );
                            }
                        } else {
                            doctor_invalid_value(
                                &file,
                                &format!("{source_path}.tokenEnv"),
                                "expected a string",
                                &mut diagnostics,
                            );
                        }
                    }
                    for (key, values) in [
                        (
                            "agent",
                            [
                                "github-copilot",
                                "claude-code",
                                "cursor",
                                "codex",
                                "gemini",
                                "antigravity",
                            ]
                            .as_slice(),
                        ),
                        ("scope", ["project", "user"].as_slice()),
                    ] {
                        if let Some(value) = source.get(key) {
                            if !value.as_str().is_some_and(|value| values.contains(&value)) {
                                doctor_invalid_value(
                                    &file,
                                    &format!("{source_path}.{key}"),
                                    format!("expected one of {}", values.join(", ")),
                                    &mut diagnostics,
                                );
                            }
                        }
                    }
                    let transport = source.get("transport").and_then(Value::as_str);
                    if (source.contains_key("registry") || source.contains_key("tokenEnv"))
                        && transport != Some("npm")
                    {
                        doctor_invalid_value(
                            &file,
                            &source_path,
                            "'registry' and 'tokenEnv' are only valid with transport 'npm'",
                            &mut diagnostics,
                        );
                    }
                    if let Some(token_env) = source.get("tokenEnv").and_then(Value::as_str) {
                        if valid_environment_name(token_env)
                            && std::env::var_os(token_env).is_none()
                        {
                            diagnostics.push(json!({
                                "severity": "warning",
                                "code": "config/token-env-not-set",
                                "file": file,
                                "message": format!("Source '{}' references environment variable '{}', which is not set.", source.get("source").and_then(Value::as_str).unwrap_or("<unnamed>"), token_env),
                                "hint": format!("Export {token_env} before running commands that fetch from this source.")
                            }));
                        }
                    }
                }
            } else {
                doctor_invalid_value(&file, "sources", "expected an array", &mut diagnostics);
            }
        }
        parsed_files.push((file_path.to_path_buf(), object));
    }
    if !path.is_file() {
        diagnostics.push(json!({
            "severity": "info",
            "code": "config/no-config-file",
            "file": display_path(&path),
            "message": "No configuration file found; carabiner will run with built-in defaults.",
            "hint": "Run 'carabiner init' to scaffold one."
        }));
    }
    let base = parsed_files
        .iter()
        .find(|(file, _)| file == &path)
        .map(|(_, value)| value);
    let local = parsed_files
        .iter()
        .find(|(file, _)| file == &local_path)
        .map(|(_, value)| value);
    if let (Some(base), Some(local)) = (base, local) {
        let targets = local.get("targets").or_else(|| base.get("targets"));
        let features = local.get("features").or_else(|| base.get("features"));
        if targets.and_then(Value::as_object).is_some() && features.is_some() {
            diagnostics.push(json!({
                "severity": "error",
                "code": "config/targets-features-conflict",
                "file": display_path(&local_path),
                "message": format!("Merging '{}' with '{}' combines object-form 'targets' with 'features', which is invalid.", display_path(&path), display_path(&local_path)),
                "hint": "Remove the conflicting field from one of the two files."
            }));
        }
    }
    let root_config = local.or(base);
    if let Some(config) = root_config {
        let file = if local.is_some() {
            display_path(&local_path)
        } else {
            display_path(&path)
        };
        let mut roots = Vec::new();
        if let Some(value) = config.get("inputRoots").and_then(Value::as_array) {
            roots.extend(
                value
                    .iter()
                    .filter_map(Value::as_str)
                    .map(ToOwned::to_owned),
            );
        } else if let Some(value) = config.get("inputRoot").and_then(Value::as_str) {
            roots.push(format!("{value}/.carabiner"));
        }
        let mut seen = HashSet::new();
        for (index, root) in roots.iter().enumerate() {
            let resolved = if Path::new(root).is_absolute() {
                PathBuf::from(root)
            } else {
                cwd.join(root)
            };
            if !seen.insert(resolved.clone()) {
                diagnostics.push(json!({"severity":"warning","code":"config/input-roots-duplicate","file":file,"message":format!("'inputRoots' contains the same directory more than once ('{}'); duplicates are ignored at generate time.", resolved.display()),"hint":"Remove the duplicate entry to make the intent explicit."}));
            }
            if !resolved.is_dir() && (index == 0 || resolved.exists()) {
                diagnostics.push(json!({"severity":"error","code":"config/input-root-not-found","file":file,"message":format!("{}'inputRoots' entry '{}' is not an existing directory.", if index == 0 { "Primary " } else { "" }, resolved.display()),"hint":"Create the directory or fix the 'inputRoots' path."}));
            }
        }
    }
    doctor_finish(options, diagnostics)
}

fn doctor_finish(options: &DoctorCli, mut diagnostics: Vec<Value>) -> Result<Value> {
    diagnostics.sort_by_key(
        |value| match value.get("severity").and_then(Value::as_str) {
            Some("error") => 0,
            Some("warning") => 1,
            _ => 2,
        },
    );
    let errors = diagnostics
        .iter()
        .filter(|value| value.get("severity").and_then(Value::as_str) == Some("error"))
        .count();
    let warnings = diagnostics
        .iter()
        .filter(|value| value.get("severity").and_then(Value::as_str) == Some("warning"))
        .count();
    let infos = diagnostics
        .iter()
        .filter(|value| value.get("severity").and_then(Value::as_str) == Some("info"))
        .count();
    if !options.silent && human_output() {
        for diagnostic in &diagnostics {
            println!(
                "{}: [{}] {}",
                diagnostic
                    .get("severity")
                    .and_then(Value::as_str)
                    .unwrap_or("info"),
                diagnostic
                    .get("code")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown"),
                diagnostic
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("")
            );
            if let Some(hint) = diagnostic.get("hint").and_then(Value::as_str) {
                println!("    ↳ {hint}");
            }
        }
    }
    let summary = json!({"errors": errors, "warnings": warnings, "infos": infos});
    if errors > 0 || (options.strict && warnings > 0) {
        return Err(Failure {
            code: "DOCTOR_FAILED",
            message: format!(
                "Doctor found problems: {errors} error(s), {warnings} warning(s), {infos} info(s)."
            ),
            exit_code: 1,
            details: Some(json!({"diagnostics": diagnostics, "summary": summary})),
        }
        .into());
    }
    if !options.silent && human_output() {
        if warnings > 0 {
            println!(
                "Doctor finished with {errors} error(s), {warnings} warning(s), {infos} info(s)."
            );
        } else {
            println!(
                "✓ No problems found ({errors} error(s), {warnings} warning(s), {infos} info(s))."
            );
        }
    }
    Ok(json!({"diagnostics": diagnostics, "summary": summary}))
}

fn check_doctor_targets(value: Option<&Value>, file: &str, diagnostics: &mut Vec<Value>) {
    let Some(value) = value else {
        return;
    };
    match value {
        Value::Array(values) => {
            for value in values {
                let Some(name) = value.as_str() else {
                    diagnostics.push(json!({
                        "severity": "error",
                        "code": "config/invalid-value",
                        "file": file,
                        "message": format!("'targets' entries must be strings, found {}.", value)
                    }));
                    continue;
                };
                if name != "*" && !all_targets().iter().any(|target| target == name) {
                    diagnostics.push(json!({
                        "severity": "error",
                        "code": "config/unknown-target",
                        "file": file,
                        "message": format!("Unknown tool target '{name}' in 'targets'."),
                        "hint": if nearest(name, &all_targets()).is_empty() { format!("Valid targets: {}.", all_targets().join(", ")) } else { nearest(name, &all_targets()) }
                    }));
                }
            }
        }
        Value::Object(object) => {
            for (name, features) in object {
                if name == "*" {
                    diagnostics.push(json!({
                        "severity": "error",
                        "code": "config/invalid-value",
                        "file": file,
                        "message": "Wildcard '*' is not supported as a key in the object form of 'targets'; per-target options cannot be attached to a wildcard.",
                        "hint": "Use the array form `\"targets\": [\"*\"]` instead."
                    }));
                    continue;
                }
                if !all_targets().iter().any(|target| target == name) {
                    diagnostics.push(json!({
                        "severity": "error",
                        "code": "config/unknown-target",
                        "file": file,
                        "message": format!("Unknown tool target '{name}' in the 'targets' object."),
                        "hint": if nearest(name, &all_targets()).is_empty() { format!("Valid targets: {}.", all_targets().join(", ")) } else { nearest(name, &all_targets()) }
                    }));
                    continue;
                }
                match features {
                    Value::Array(values) => {
                        for feature in values {
                            let Some(feature) = feature.as_str() else {
                                diagnostics.push(json!({"severity":"error","code":"config/invalid-value","file":file,"message":format!("Features for target '{name}' must be strings, found {feature}."),}));
                                continue;
                            };
                            if feature == "ignore" {
                                diagnostics.push(json!({"severity":"warning","code":"config/deprecated-feature","file":file,"message":format!("Feature 'ignore' in 'targets.{name}' is deprecated."),"hint":"Use the 'permissions' feature instead."}));
                            } else if feature != "*" && !all_features().iter().any(|known| known == feature) {
                                diagnostics.push(json!({"severity":"error","code":"config/unknown-feature","file":file,"message":format!("Unknown feature '{feature}' in 'targets.{name}'."),"hint":if nearest(feature,&all_features()).is_empty(){format!("Valid features: {}.",all_features().join(", "))}else{nearest(feature,&all_features())}}));
                            }
                        }
                    }
                    Value::Object(object) => {
                        for feature in object.keys() {
                            if feature == "*" || feature == "gitignoreDestination" {
                                continue;
                            }
                            if feature == "ignore" {
                                diagnostics.push(json!({"severity":"warning","code":"config/deprecated-feature","file":file,"message":format!("Feature 'ignore' in 'targets.{name}' is deprecated."),"hint":"Use the 'permissions' feature instead."}));
                            } else if !all_features().iter().any(|known| known == feature) {
                                diagnostics.push(json!({"severity":"error","code":"config/unknown-feature","file":file,"message":format!("Unknown feature '{feature}' in 'targets.{name}'."),"hint":if nearest(feature,&all_features()).is_empty(){format!("Valid features: {}.",all_features().join(", "))}else{nearest(feature,&all_features())}}));
                            }
                        }
                    }
                    other => diagnostics.push(json!({"severity":"error","code":"config/invalid-value","file":file,"message":format!("Value for target '{name}' must be a feature array or a per-feature object, found {other}."),})),
                }
            }
        }
        other => diagnostics.push(json!({"severity":"error","code":"config/invalid-value","file":file,"message":format!("'targets' must be an array of tool names or a per-target object, found {other}."),})),
    }
}

fn check_doctor_features(value: Option<&Value>, file: &str, diagnostics: &mut Vec<Value>) {
    let Some(value) = value else {
        return;
    };
    let Value::Array(values) = value else {
        diagnostics.push(json!({"severity":"error","code":"config/invalid-value","file":file,"message":format!("'features' must be an array of feature names, found {value}."),"hint":"To configure features per target, use the object form of 'targets' instead."}));
        return;
    };
    for value in values {
        let Some(name) = value.as_str() else {
            diagnostics.push(json!({"severity":"error","code":"config/invalid-value","file":file,"message":format!("'features' entries must be strings, found {value}."),}));
            continue;
        };
        if name == "ignore" {
            diagnostics.push(json!({"severity":"warning","code":"config/deprecated-feature","file":file,"message":"Feature 'ignore' is deprecated.","hint":"Use the 'permissions' feature instead."}));
        } else if name != "*" && !all_features().iter().any(|feature| feature == name) {
            diagnostics.push(json!({"severity":"error","code":"config/unknown-feature","file":file,"message":format!("Unknown feature '{name}'."),"hint":if nearest(name,&all_features()).is_empty(){format!("Valid features: {}.",all_features().join(", "))}else{nearest(name,&all_features())}}));
        }
    }
}

fn nearest(input: &str, candidates: &[impl AsRef<str>]) -> String {
    let mut best = None;
    let mut distance = usize::MAX;
    for candidate in candidates {
        let candidate = candidate.as_ref();
        let current = levenshtein(&input.to_ascii_lowercase(), &candidate.to_ascii_lowercase());
        if current < distance {
            distance = current;
            best = Some(candidate);
        }
    }
    if distance <= std::cmp::max(2, input.len() / 3) {
        format!("Did you mean '{}'{}", best.unwrap_or(""), "?")
    } else {
        String::new()
    }
}

fn levenshtein(left: &str, right: &str) -> usize {
    let mut previous = (0..=right.chars().count()).collect::<Vec<_>>();
    for (i, a) in left.chars().enumerate() {
        let mut current = vec![i + 1];
        for (j, b) in right.chars().enumerate() {
            current.push(std::cmp::min(
                std::cmp::min(previous[j + 1] + 1, current[j] + 1),
                previous[j] + usize::from(a != b),
            ));
        }
        previous = current;
    }
    *previous.last().unwrap_or(&0)
}

const DOCS_CONTENT: &[(&str, &str)] = &[
    (
        "api/programmatic-api",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/docs/api/programmatic-api.md"
        )),
    ),
    (
        "faq",
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/docs/faq.md")),
    ),
    (
        "getting-started/installation",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/docs/getting-started/installation.md"
        )),
    ),
    (
        "getting-started/quick-start",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/docs/getting-started/quick-start.md"
        )),
    ),
    (
        "guide/case-studies",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/docs/guide/case-studies.md"
        )),
    ),
    (
        "guide/configuration",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/docs/guide/configuration.md"
        )),
    ),
    (
        "guide/declarative-sources",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/docs/guide/declarative-sources.md"
        )),
    ),
    (
        "guide/dry-run",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/docs/guide/dry-run.md"
        )),
    ),
    (
        "guide/global-mode",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/docs/guide/global-mode.md"
        )),
    ),
    (
        "guide/official-skills",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/docs/guide/official-skills.md"
        )),
    ),
    (
        "guide/plugin-packaging",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/docs/guide/plugin-packaging.md"
        )),
    ),
    (
        "guide/separate-input-root",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/docs/guide/separate-input-root.md"
        )),
    ),
    (
        "guide/simulated-features",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/docs/guide/simulated-features.md"
        )),
    ),
    (
        "guide/why-carabiner",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/docs/guide/why-carabiner.md"
        )),
    ),
    (
        "index",
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/docs/index.md")),
    ),
    (
        "reference/cli-commands",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/docs/reference/cli-commands.md"
        )),
    ),
    (
        "reference/command-syntax",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/docs/reference/command-syntax.md"
        )),
    ),
    (
        "reference/file-formats",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/docs/reference/file-formats.md"
        )),
    ),
    (
        "reference/mcp-server",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/docs/reference/mcp-server.md"
        )),
    ),
    (
        "reference/supported-tools",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/docs/reference/supported-tools.md"
        )),
    ),
    (
        "tools/takt",
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/docs/tools/takt.md")),
    ),
];

fn normalize_doc_id(input: &str) -> Option<String> {
    let slashed = input.trim().replace('\\', "/");
    if slashed.is_empty()
        || slashed.starts_with('/')
        || slashed.as_bytes().get(1).is_some_and(|byte| *byte == b':')
    {
        return None;
    }
    let mut segments = slashed
        .split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .collect::<Vec<_>>();
    if segments.contains(&"..") {
        return None;
    }
    if segments.first().copied() == Some("docs") {
        segments.remove(0);
    }
    if segments.is_empty() {
        return None;
    }
    let joined = segments.join("/");
    Some(joined.strip_suffix(".md").unwrap_or(&joined).to_owned())
}

fn docs_context(content: &str, terms: &[String]) -> String {
    content
        .lines()
        .find(|line| {
            let lower = line.to_ascii_lowercase();
            terms.iter().any(|term| lower.contains(term))
        })
        .map(|line| {
            let trimmed = line.trim();
            if trimmed.chars().count() > 160 {
                let short = trimmed.chars().take(157).collect::<String>();
                format!("{short}...")
            } else {
                trimmed.to_owned()
            }
        })
        .unwrap_or_else(|| content.lines().next().unwrap_or("").trim().to_owned())
}

fn docs_search_score(id: &str, content: &str, terms: &[String]) -> usize {
    let lower_id = id.to_ascii_lowercase();
    let title = content
        .lines()
        .find(|line| line.starts_with("# "))
        .unwrap_or("")
        .to_ascii_lowercase();
    let headings = content
        .lines()
        .filter(|line| line.starts_with("## ") || line.starts_with("### "))
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>()
        .join("\n");
    let body = content.to_ascii_lowercase();
    terms
        .iter()
        .map(|term| {
            let mut score = 0;
            if lower_id.contains(term) {
                score += 2;
            }
            if title.contains(term) {
                score += 4;
            }
            if headings.contains(term) {
                score += 3;
            }
            if body.contains(term) {
                score += 1;
            }
            score
        })
        .sum()
}

fn docs_command(options: &DocsCli) -> Result<Value> {
    if options.search.is_some() && options.document.is_some() {
        return Err(anyhow!(
            "Specify either a document or --search <text>, not both."
        ));
    }
    let mut documents = DOCS_CONTENT.iter().map(|(id, _)| *id).collect::<Vec<_>>();
    documents.sort_unstable();
    if let Some(query) = &options.search {
        let query = query.trim();
        if query.is_empty() {
            return Err(anyhow!("--search requires a non-empty search text."));
        }
        let terms = query
            .to_ascii_lowercase()
            .split_whitespace()
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        let mut matches = DOCS_CONTENT
            .iter()
            .filter_map(|(id, content)| {
                let score = docs_search_score(id, content, &terms);
                (score > 0).then_some((*id, *content, score))
            })
            .collect::<Vec<_>>();
        matches.sort_by(|left, right| right.2.cmp(&left.2).then_with(|| left.0.cmp(right.0)));
        if matches.is_empty() {
            return Err(anyhow!(
                "No documents match '{query}'. Run 'carabiner docs' to list documents."
            ));
        }
        let results = matches
            .into_iter()
            .take(10)
            .map(|(id, content, _)| format!("{id} — {}", docs_context(content, &terms)))
            .collect::<Vec<_>>();
        for line in &results {
            println!("{line}");
        }
        return Ok(json!({"matches": results}));
    }
    let Some(document) = &options.document else {
        for id in &documents {
            println!("{id}");
        }
        return Ok(json!({"documents": documents}));
    };
    let id = normalize_doc_id(document)
        .ok_or_else(|| anyhow!("Invalid document identifier: '{document}'."))?;
    let content = DOCS_CONTENT
        .iter()
        .find(|(candidate, _)| *candidate == id)
        .map(|(_, content)| *content)
        .ok_or_else(|| {
            anyhow!("Unknown document '{id}'. Run 'carabiner docs' to list documents.")
        })?;
    print!("{content}");
    Ok(json!({"document": id}))
}

fn github_releases(source: &str, token: Option<&str>) -> Result<Value> {
    let token = token
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            std::env::var("GITHUB_TOKEN")
                .ok()
                .filter(|value| !value.is_empty())
        })
        .or_else(|| {
            std::env::var("GH_TOKEN")
                .ok()
                .filter(|value| !value.is_empty())
        });
    let source = source
        .trim()
        .trim_start_matches("https://github.com/")
        .trim_end_matches('/')
        .trim_end_matches(".git");
    if source.split('/').count() != 2 {
        return Err(anyhow!("GitHub source must be owner/repo"));
    }
    let api_base = std::env::var("CARABINER_UPDATE_API_BASE")
        .unwrap_or_else(|_| "https://api.github.com".into());
    if api_base.contains('\n') || api_base.contains('\r') {
        return Err(anyhow!(
            "CARABINER_UPDATE_API_BASE must not contain newlines"
        ));
    }
    let url = format!(
        "{}/repos/{source}/releases?per_page=100",
        api_base.trim_end_matches('/')
    );
    let mut command = Command::new("curl");
    command.args([
        "-fsSL",
        "-H",
        "Accept: application/vnd.github+json",
        "-H",
        "User-Agent: carabiner-rust",
    ]);
    if let Some(token) = token {
        command.args(["-H", &format!("Authorization: Bearer {token}")]);
    }
    command.arg(url);
    let output = command.output().context("failed to start curl")?;
    if !output.status.success() {
        return Err(anyhow!("GitHub releases request failed"));
    }
    serde_json::from_slice(&output.stdout).context("invalid GitHub releases response")
}

fn parse_release_repository(source: &str) -> Result<(String, String)> {
    let mut value = source.trim().to_owned();
    for prefix in ["https://github.com/", "http://github.com/"] {
        if let Some(rest) = value.strip_prefix(prefix) {
            value = rest.to_owned();
            break;
        }
    }
    value = value
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .to_owned();
    if value.contains('@') || value.contains(':') {
        return Err(anyhow!(
            "Invalid repository \"{source}\". Expected owner/repo without a ref (\"@\") or path (\":\") suffix; use --tag to select a single release."
        ));
    }
    let mut parts = value.split('/');
    let owner = parts.next().filter(|part| !part.is_empty());
    let repo = parts.next().filter(|part| !part.is_empty());
    if owner.is_none() || repo.is_none() || parts.next().is_some() {
        return Err(anyhow!("GitHub source must be owner/repo"));
    }
    Ok((
        owner.expect("checked Some above").to_owned(),
        repo.expect("checked Some above").to_owned(),
    ))
}

fn release_date(value: &str, end_of_day: bool) -> Result<chrono::DateTime<chrono::Utc>> {
    let value = value.trim();
    if end_of_day && value.len() == 10 {
        let date = chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")?;
        return Ok(date
            .and_hms_milli_opt(23, 59, 59, 999)
            .ok_or_else(|| anyhow!("invalid date"))?
            .and_utc());
    }
    Ok(chrono::DateTime::parse_from_rfc3339(value)?.with_timezone(&chrono::Utc))
}

fn release_note(value: &Value) -> Value {
    json!({
        "tagName": value.get("tag_name").and_then(Value::as_str).unwrap_or(""),
        "name": value.get("name").and_then(Value::as_str),
        "publishedAt": value.get("published_at").and_then(Value::as_str),
        "prerelease": value.get("prerelease").and_then(Value::as_bool).unwrap_or(false),
        "url": value.get("html_url").and_then(Value::as_str),
        "body": value.get("body").and_then(Value::as_str),
    })
}

fn release_filter_value(options: &ReleaseNotesCli) -> Result<Value> {
    let mut modes = Vec::new();
    if options.latest.is_some() {
        modes.push("--latest");
    }
    if options.since.is_some() || options.until.is_some() {
        modes.push("--since/--until");
    }
    if options.tag.is_some() {
        modes.push("--tag");
    }
    if options.from.is_some() || options.to.is_some() {
        modes.push("--from/--to");
    }
    if modes.len() > 1 {
        return Err(anyhow!(
            "Conflicting filter options: {}. Use only one of --latest, --since/--until, --tag, or --from/--to.",
            modes.join(", ")
        ));
    }
    if let Some(raw) = &options.latest {
        let count = raw
            .trim()
            .parse::<usize>()
            .ok()
            .filter(|count| *count > 0)
            .ok_or_else(|| {
                anyhow!("Invalid --latest value \"{raw}\". Expected a positive integer.")
            })?;
        return Ok(json!({"kind": "latest", "count": count}));
    }
    if options.since.is_some() || options.until.is_some() {
        if let Some(since) = &options.since {
            release_date(since, false).map_err(|_| {
                anyhow!("Invalid --since value \"{since}\". Expected a date such as 2026-01-31.")
            })?;
        }
        if let Some(until) = &options.until {
            release_date(until, true).map_err(|_| {
                anyhow!("Invalid --until value \"{until}\". Expected a date such as 2026-01-31.")
            })?;
        }
        return Ok(json!({"kind": "dateRange", "since": options.since, "until": options.until}));
    }
    if let Some(tag) = &options.tag {
        return Ok(json!({"kind": "singleTag", "tag": tag}));
    }
    if options.from.is_some() || options.to.is_some() {
        let from = options.from.as_deref().ok_or_else(|| {
            anyhow!("Both --from and --to are required to select a version range.")
        })?;
        let to = options.to.as_deref().ok_or_else(|| {
            anyhow!("Both --from and --to are required to select a version range.")
        })?;
        return Ok(json!({"kind": "tagRange", "from": from, "to": to}));
    }
    Ok(json!({"kind": "latest", "count": 10}))
}

fn release_published_at(value: &Value) -> Option<chrono::DateTime<chrono::Utc>> {
    value
        .get("published_at")
        .and_then(Value::as_str)
        .and_then(|value| release_date(value, false).ok())
}

fn release_notes_command(options: &ReleaseNotesCli) -> Result<Value> {
    let (owner, repo) = parse_release_repository(&options.source)?;
    let filter = release_filter_value(options)?;
    let value = github_releases(&format!("{owner}/{repo}"), options.token.as_deref())?;
    let releases = value
        .as_array()
        .ok_or_else(|| anyhow!("GitHub releases response was not an array"))?;
    let kind = filter
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("latest");
    let mut selected = Vec::new();
    match kind {
        "singleTag" => {
            let tag = filter.get("tag").and_then(Value::as_str).unwrap_or("");
            if let Some(release) = releases
                .iter()
                .find(|release| release.get("tag_name").and_then(Value::as_str) == Some(tag))
            {
                if release.get("draft").and_then(Value::as_bool) != Some(true) {
                    selected.push(release.clone());
                }
            }
        }
        "tagRange" => {
            let from = filter.get("from").and_then(Value::as_str).unwrap_or("");
            let to = filter.get("to").and_then(Value::as_str).unwrap_or("");
            let first = releases
                .iter()
                .position(|release| release.get("tag_name").and_then(Value::as_str) == Some(from));
            let second = releases
                .iter()
                .position(|release| release.get("tag_name").and_then(Value::as_str) == Some(to));
            let (Some(first), Some(second)) = (first, second) else {
                let missing = if first.is_none() { from } else { to };
                return Err(anyhow!(
                    "Release tag \"{missing}\" was not found in {owner}/{repo} within the most recent 100 releases."
                ));
            };
            let (start, end) = if first <= second {
                (first, second)
            } else {
                (second, first)
            };
            selected.extend(
                releases[start..=end]
                    .iter()
                    .filter(|release| release.get("draft").and_then(Value::as_bool) != Some(true))
                    .filter(|release| {
                        options.include_prereleases
                            || release.get("prerelease").and_then(Value::as_bool) != Some(true)
                    })
                    .cloned(),
            );
        }
        "dateRange" => {
            let since = filter
                .get("since")
                .and_then(Value::as_str)
                .map(|value| release_date(value, false))
                .transpose()?;
            let until = filter
                .get("until")
                .and_then(Value::as_str)
                .map(|value| release_date(value, true))
                .transpose()?;
            selected.extend(
                releases
                    .iter()
                    .filter(|release| release.get("draft").and_then(Value::as_bool) != Some(true))
                    .filter(|release| {
                        options.include_prereleases
                            || release.get("prerelease").and_then(Value::as_bool) != Some(true)
                    })
                    .filter(|release| {
                        let Some(published) = release_published_at(release) else {
                            return false;
                        };
                        since.as_ref().is_none_or(|value| published >= *value)
                            && until.as_ref().is_none_or(|value| published <= *value)
                    })
                    .cloned(),
            );
        }
        _ => {
            let count = filter.get("count").and_then(Value::as_u64).unwrap_or(10) as usize;
            selected.extend(
                releases
                    .iter()
                    .filter(|release| release.get("draft").and_then(Value::as_bool) != Some(true))
                    .filter(|release| {
                        options.include_prereleases
                            || release.get("prerelease").and_then(Value::as_bool) != Some(true)
                    })
                    .take(count)
                    .cloned(),
            );
        }
    }
    let notes = selected.iter().map(release_note).collect::<Vec<_>>();
    if !options.silent && human_output() {
        println!("# Release notes for {owner}/{repo}\n");
        for note in &notes {
            let tag = note.get("tagName").and_then(Value::as_str).unwrap_or("");
            let name = note.get("name").and_then(Value::as_str);
            let title = name.filter(|name| *name != tag);
            println!(
                "## {}{}\n",
                tag,
                title.map(|name| format!(" — {name}")).unwrap_or_default()
            );
            let date = note
                .get("publishedAt")
                .and_then(Value::as_str)
                .map(|value| value.split('T').next().unwrap_or(value))
                .unwrap_or("unpublished");
            let mut metadata = format!("Published: {date}");
            if note.get("prerelease").and_then(Value::as_bool) == Some(true) {
                metadata.push_str(" | Prerelease");
            }
            if let Some(url) = note.get("url").and_then(Value::as_str) {
                metadata.push_str(" | ");
                metadata.push_str(url);
            }
            println!("{metadata}\n");
            let body = note
                .get("body")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            println!(
                "{}\n",
                if body.is_empty() {
                    "_No release notes._"
                } else {
                    body
                }
            );
        }
    }
    Ok(json!({
        "repository": format!("{owner}/{repo}"),
        "filter": filter,
        "releases": notes,
        "totalReleases": selected.len(),
    }))
}

fn normalize_version(value: &str) -> String {
    value
        .trim()
        .trim_start_matches('v')
        .split('-')
        .next()
        .unwrap_or("")
        .to_owned()
}

fn compare_versions(left: &str, right: &str) -> Result<std::cmp::Ordering> {
    let left = normalize_version(left);
    let right = normalize_version(right);
    let left_parts = left.split('.').collect::<Vec<_>>();
    let right_parts = right.split('.').collect::<Vec<_>>();
    let length = left_parts.len().max(right_parts.len());
    for index in 0..length {
        let left_part = left_parts.get(index).copied().unwrap_or("0");
        let right_part = right_parts.get(index).copied().unwrap_or("0");
        let left_number = left_part.parse::<u64>().map_err(|_| {
            anyhow!("Invalid version format: cannot compare \"{left}\" and \"{right}\"")
        })?;
        let right_number = right_part.parse::<u64>().map_err(|_| {
            anyhow!("Invalid version format: cannot compare \"{left}\" and \"{right}\"")
        })?;
        match left_number.cmp(&right_number) {
            std::cmp::Ordering::Equal => {}
            ordering => return Ok(ordering),
        }
    }
    Ok(std::cmp::Ordering::Equal)
}

fn update_repository(options: &UpdateCli) -> Result<String> {
    let env_repository = std::env::var("CARABINER_UPDATE_REPOSITORY").ok();
    let source = options
        .repository
        .as_deref()
        .or(env_repository.as_deref())
        .ok_or_else(|| {
            anyhow!(
                "Carabiner update repository is not configured. Pass --repository owner/repo or set CARABINER_UPDATE_REPOSITORY."
            )
        })?;
    let (owner, repo) = parse_release_repository(source)?;
    Ok(format!("{owner}/{repo}"))
}

fn update_asset_prefix(options: &UpdateCli) -> Result<String> {
    let env_prefix = std::env::var("CARABINER_UPDATE_ASSET_PREFIX").ok();
    let prefix = options
        .asset_prefix
        .as_deref()
        .or(env_prefix.as_deref())
        .unwrap_or("carabiner");
    if prefix.is_empty()
        || !prefix.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
    {
        return Err(anyhow!(
            "Invalid Carabiner update asset prefix '{prefix}'. Use only letters, numbers, '-', '_', or '.'."
        ));
    }
    Ok(prefix.to_owned())
}

fn update_asset_name(prefix: &str) -> Result<String> {
    let platform = std::env::consts::OS;
    let architecture = std::env::consts::ARCH;
    let platform_name = match platform {
        "macos" => "darwin",
        "windows" => "windows",
        "linux" => "linux",
        _ => {
            return Err(anyhow!(
                "Unsupported platform: {platform} {architecture}. Configure a compatible Carabiner release asset manually."
            ));
        }
    };
    let architecture_name = match architecture {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        _ => {
            return Err(anyhow!(
                "Unsupported platform: {platform} {architecture}. Configure a compatible Carabiner release asset manually."
            ));
        }
    };
    let extension = if platform == "windows" { ".exe" } else { "" };
    Ok(format!(
        "{prefix}-{platform_name}-{architecture_name}{extension}"
    ))
}

fn github_asset<'a>(release: &'a Value, name: &str) -> Option<&'a Value> {
    release
        .get("assets")
        .and_then(Value::as_array)
        .and_then(|assets| {
            assets
                .iter()
                .find(|asset| asset.get("name").and_then(Value::as_str) == Some(name))
        })
}

fn trusted_update_url(url: &str, repository: &str) -> bool {
    let github_prefix = format!("https://github.com/{repository}/");
    let custom_prefix = std::env::var("CARABINER_UPDATE_DOWNLOAD_BASE")
        .ok()
        .filter(|base| !base.contains('\n') && !base.contains('\r'))
        .map(|base| format!("{}/", base.trim_end_matches('/')));
    let custom_allowed = custom_prefix
        .as_deref()
        .is_some_and(|prefix| url.starts_with(prefix));
    (url.starts_with(&github_prefix)
        || url.starts_with("https://objects.githubusercontent.com/")
        || url.starts_with("https://github-releases.githubusercontent.com/")
        || custom_allowed)
        && !url.contains('\n')
        && !url.contains('\r')
}
fn download_update_asset(
    url: &str,
    destination: &Path,
    token: Option<&str>,
    repository: &str,
) -> Result<()> {
    if !trusted_update_url(url, repository) {
        return Err(anyhow!("Untrusted update download URL: {url}"));
    }
    let mut command = Command::new("curl");
    command.args([
        "--fail",
        "--location",
        "--silent",
        "--show-error",
        "--retry",
        "2",
        "--connect-timeout",
        "15",
        "--max-time",
        "300",
        "--output",
        destination.to_string_lossy().as_ref(),
        url,
    ]);
    if let Some(token) = token.filter(|value| !value.is_empty()) {
        command.args(["--header", &format!("Authorization: Bearer {token}")]);
    }
    let status = command.status().context("failed to start curl")?;
    if !status.success() {
        return Err(anyhow!("Failed to download update asset from {url}"));
    }
    Ok(())
}

fn checksum_for_asset(checksums: &[u8], asset_name: &str) -> Option<String> {
    String::from_utf8_lossy(checksums).lines().find_map(|line| {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() >= 2
            && fields.last().map(|value| value.trim_start_matches('*')) == Some(asset_name)
            && fields[0].len() == 64
            && fields[0].chars().all(|value| value.is_ascii_hexdigit())
        {
            Some(fields[0].to_ascii_lowercase())
        } else {
            None
        }
    })
}

fn install_update_binary(
    temp_dir: &Path,
    downloaded: &Path,
    current_version: &str,
    latest_version: &str,
) -> Result<String> {
    let current = std::env::current_exe()
        .context("failed to locate current executable")?
        .canonicalize()
        .context("failed to resolve current executable")?;
    let current_dir = current
        .parent()
        .ok_or_else(|| anyhow!("current executable has no parent directory"))?;
    let backup = temp_dir.join("carabiner.backup");
    fs::copy(&current, &backup).with_context(|| {
        format!(
            "Permission denied: cannot read {}. Try running with sudo.",
            current.display()
        )
    })?;
    let in_place = current_dir.join(format!(".carabiner-update-{}", std::process::id()));
    fs::copy(downloaded, &in_place).context("failed to stage updated executable")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&in_place, fs::Permissions::from_mode(0o755))?;
    }
    let replacement = match fs::rename(&in_place, &current) {
        Ok(()) => Ok(()),
        Err(rename_error) => match fs::copy(downloaded, &current) {
            Ok(_) => {
                let _ = fs::remove_file(&in_place);
                Ok(())
            }
            Err(copy_error) => Err(anyhow!(
                "{rename_error}; fallback copy failed: {copy_error}"
            )),
        },
    };
    if let Err(error) = replacement {
        let restore = fs::copy(&backup, &current);
        if let Err(restore_error) = restore {
            return Err(anyhow!(
                "Failed to replace binary and restore failed. Backup is preserved at {}. Original error: {error}; restore error: {restore_error}",
                backup.display()
            ));
        }
        return Err(error.context(format!(
            "Permission denied: cannot write to {}. Try running with sudo.",
            current_dir.display()
        )));
    }
    Ok(format!(
        "Successfully updated from {current_version} to {latest_version}"
    ))
}

fn perform_binary_update(
    release: &Value,
    current_version: &str,
    latest_version: &str,
    repository: &str,
    asset_prefix: &str,
    token: Option<&str>,
) -> Result<String> {
    let asset_name = update_asset_name(asset_prefix)?;
    let release_url = format!("https://github.com/{repository}/releases");
    let binary_asset = github_asset(release, &asset_name).ok_or_else(|| {
        anyhow!("Binary for {asset_name} not found in release. Please download manually from {release_url}")
    })?;
    let checksum_asset = github_asset(release, "SHA256SUMS").ok_or_else(|| {
        anyhow!("SHA256SUMS not found in release. Cannot verify download integrity. Please download manually from {release_url}")
    })?;
    let binary_url = binary_asset
        .get("browser_download_url")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("Update binary asset has no download URL"))?;
    let checksum_url = checksum_asset
        .get("browser_download_url")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("SHA256SUMS asset has no download URL"))?;
    let temp_dir = std::env::temp_dir().join(format!(
        "carabiner-update-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    fs::create_dir(&temp_dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temp_dir, fs::Permissions::from_mode(0o700))?;
    }
    let result = (|| {
        let binary_path = temp_dir.join(&asset_name);
        let checksums_path = temp_dir.join("SHA256SUMS");
        download_update_asset(binary_url, &binary_path, token, repository)?;
        download_update_asset(checksum_url, &checksums_path, token, repository)?;
        let binary = fs::read(&binary_path)?;
        if binary.len() > 500 * 1024 * 1024 {
            return Err(anyhow!("Update binary exceeds maximum 500MB size"));
        }
        let checksums = fs::read(&checksums_path)?;
        let expected = checksum_for_asset(&checksums, &asset_name).ok_or_else(|| {
            anyhow!("Checksum entry for \"{asset_name}\" not found in SHA256SUMS. Cannot verify download integrity.")
        })?;
        let actual = sha256_bytes(&binary)
            .strip_prefix("sha256-")
            .unwrap_or("")
            .to_ascii_lowercase();
        if actual != expected {
            return Err(anyhow!(
                "Checksum verification failed. Expected: {expected}, Got: {actual}. The download may be corrupted."
            ));
        }
        install_update_binary(&temp_dir, &binary_path, current_version, latest_version)
    })();
    let preserve_temp = result.as_ref().err().is_some_and(|error| {
        error
            .to_string()
            .to_ascii_lowercase()
            .contains("restore failed")
    });
    if !preserve_temp {
        fs::remove_dir_all(&temp_dir)?;
    }
    result
}

fn update_command(options: &UpdateCli) -> Result<Value> {
    let repository = update_repository(options)?;
    let asset_prefix = update_asset_prefix(options)?;
    let value = github_releases(&repository, options.token.as_deref())?;
    let latest = value
        .as_array()
        .and_then(|items| {
            items.iter().find(|item| {
                item.get("draft").and_then(Value::as_bool) != Some(true)
                    && item.get("prerelease").and_then(Value::as_bool) != Some(true)
            })
        })
        .and_then(|item| item.get("tag_name"))
        .and_then(Value::as_str)
        .unwrap_or(VERSION)
        .to_owned();
    let current_version = normalize_version(VERSION);
    let latest_version = normalize_version(&latest);
    let has_update =
        compare_versions(&latest_version, &current_version)? == std::cmp::Ordering::Greater;
    if options.check {
        let message = if has_update {
            format!("Update available: {current_version} -> {latest_version}")
        } else {
            format!("Already at the latest version ({current_version})")
        };
        if !options.silent && human_output() {
            println!("{message}");
        }
        return Ok(json!({
            "currentVersion": current_version,
            "latestVersion": latest_version,
            "updateAvailable": has_update,
            "message": message,
        }));
    }
    if !has_update && !options.force {
        let message = format!("Already at the latest version ({current_version})");
        if !options.silent && human_output() {
            println!("{message}");
        }
        return Ok(json!({
            "currentVersion": current_version,
            "latestVersion": latest_version,
            "updateAvailable": has_update,
            "updated": false,
            "message": message,
        }));
    }
    let message = perform_binary_update(
        value
            .as_array()
            .and_then(|items| {
                items.iter().find(|item| {
                    item.get("tag_name").and_then(Value::as_str) == Some(latest.as_str())
                })
            })
            .unwrap_or(&Value::Null),
        &current_version,
        &latest_version,
        &repository,
        &asset_prefix,
        options.token.as_deref(),
    )?;
    if !options.silent && human_output() {
        println!("{message}");
    }
    Ok(json!({
        "currentVersion": current_version,
        "latestVersion": latest_version,
        "updateAvailable": has_update,
        "updated": true,
        "message": message,
    }))
}

fn mcp_required_string(args: &Map<String, Value>, key: &str) -> Result<String> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| anyhow!("{key} is required"))
}

fn mcp_feature_candidates(feature: &str) -> Option<&'static [&'static str]> {
    match feature {
        "ignore" => Some(&[".carabiner/.aiignore", ".carabinerignore"]),
        "mcp" => Some(&[
            ".carabiner/mcp.jsonc",
            ".carabiner/mcp.json",
            ".carabiner/.mcp.json",
        ]),
        "permissions" => Some(&[
            ".carabiner/permissions.jsonc",
            ".carabiner/permissions.json",
        ]),
        "hooks" => Some(&[".carabiner/hooks.jsonc", ".carabiner/hooks.json"]),
        _ => None,
    }
}

fn mcp_singleton_path(feature: &str) -> Result<String> {
    let candidates = mcp_feature_candidates(feature)
        .ok_or_else(|| anyhow!("unknown MCP feature '{feature}'"))?;
    Ok(candidates
        .iter()
        .find(|candidate| Path::new(candidate).is_file())
        .copied()
        .unwrap_or(candidates[0])
        .into())
}

fn mcp_markdown_dir(feature: &str) -> Option<&'static str> {
    match feature {
        "rule" => Some(".carabiner/rules"),
        "command" => Some(".carabiner/commands"),
        "subagent" => Some(".carabiner/subagents"),
        "skill" => Some(".carabiner/skills"),
        "check" => Some(".carabiner/checks"),
        _ => None,
    }
}

fn mcp_frontmatter(feature: &str, value: Option<&Value>) -> Result<Map<String, Value>> {
    let mut frontmatter = value
        .cloned()
        .unwrap_or_else(|| Value::Object(Map::new()))
        .as_object()
        .cloned()
        .ok_or_else(|| anyhow!("frontmatter must be an object"))?;
    let targets = match frontmatter.get("targets") {
        None => vec![Value::String("*".into())],
        Some(Value::Array(values)) => values
            .iter()
            .map(|target| {
                let target = target
                    .as_str()
                    .ok_or_else(|| anyhow!("frontmatter.targets entries must be strings"))?;
                if target != "*" && target_spec(target).is_none() {
                    return Err(anyhow!("Invalid target '{target}' in frontmatter.targets"));
                }
                Ok(Value::String(target.into()))
            })
            .collect::<Result<Vec<_>>>()?,
        _ => return Err(anyhow!("frontmatter.targets must be an array")),
    };
    frontmatter.insert("targets".into(), Value::Array(targets));
    if let Some(description) = frontmatter.get("description") {
        if !description.is_string() {
            return Err(anyhow!("frontmatter.description must be a string"));
        }
    }
    for key in ["root", "localRoot"] {
        if let Some(value) = frontmatter.get(key) {
            if !value.is_boolean() {
                return Err(anyhow!("frontmatter.{key} must be a boolean"));
            }
        }
    }
    if let Some(globs) = frontmatter.get("globs") {
        let values = globs
            .as_array()
            .ok_or_else(|| anyhow!("frontmatter.globs must be an array"))?;
        if values.iter().any(|value| !value.is_string()) {
            return Err(anyhow!("frontmatter.globs entries must be strings"));
        }
    }
    match feature {
        "subagent" => {
            if frontmatter.get("name").and_then(Value::as_str).is_none() {
                return Err(anyhow!("frontmatter.name is required for subagent"));
            }
        }
        "skill" => {
            let name = frontmatter
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("frontmatter.name is required for skill"))?;
            safe_name(name)?;
            if frontmatter
                .get("description")
                .and_then(Value::as_str)
                .is_none()
            {
                return Err(anyhow!("frontmatter.description is required for skill"));
            }
        }
        "check" => {
            if let Some(severity) = frontmatter.get("severity") {
                let severity = severity
                    .as_str()
                    .ok_or_else(|| anyhow!("frontmatter.severity must be a string"))?;
                if !matches!(severity, "low" | "medium" | "high" | "critical") {
                    return Err(anyhow!("Invalid check severity '{severity}'"));
                }
            }
            if let Some(tools) = frontmatter.get("tools") {
                let values = tools
                    .as_array()
                    .ok_or_else(|| anyhow!("frontmatter.tools must be an array"))?;
                if values.iter().any(|value| !value.is_string()) {
                    return Err(anyhow!("frontmatter.tools entries must be strings"));
                }
            }
        }
        _ => {}
    }
    Ok(frontmatter)
}

fn encode_base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let first = chunk[0] as usize;
        let second = chunk.get(1).copied().unwrap_or(0) as usize;
        let third = chunk.get(2).copied().unwrap_or(0) as usize;
        output.push(ALPHABET[first >> 2] as char);
        output.push(ALPHABET[((first & 0x03) << 4) | (second >> 4)] as char);
        if chunk.len() > 1 {
            output.push(ALPHABET[((second & 0x0f) << 2) | (third >> 6)] as char);
        } else {
            output.push('=');
        }
        if chunk.len() > 2 {
            output.push(ALPHABET[third & 0x3f] as char);
        } else {
            output.push('=');
        }
    }
    output
}

fn mcp_skill_other_files(skill_root: &Path) -> Result<Vec<Value>> {
    let mut files = Vec::new();
    for path in walk_files(skill_root) {
        let name = relative_slash(skill_root, &path);
        safe_relative_path(&name)?;
        if name == "SKILL.md" {
            continue;
        }
        let bytes = fs::read(&path)?;
        if let Ok(content) = std::str::from_utf8(&bytes) {
            files.push(json!({"name": name, "body": content, "encoding": "utf-8"}));
        } else {
            files.push(json!({"name": name, "body": encode_base64(&bytes), "encoding": "base64"}));
        }
    }
    files.sort_by(|left, right| {
        left.get("name")
            .and_then(Value::as_str)
            .cmp(&right.get("name").and_then(Value::as_str))
    });
    Ok(files)
}

fn mcp_skill_path_is_safe(skill_root: &Path, target: &Path) -> Result<()> {
    let mut current = PathBuf::new();
    for component in target.components() {
        current.push(component.as_os_str());
        if fs::symlink_metadata(&current)
            .map(|metadata| metadata.file_type().is_symlink())
            .unwrap_or(false)
        {
            return Err(anyhow!(
                "Refusing to write through a symbolic link: {}",
                current.display()
            ));
        }
        if current == skill_root {
            break;
        }
    }
    Ok(())
}

fn mcp_decode_skill_files(args: &Map<String, Value>) -> Result<Vec<(String, Vec<u8>)>> {
    let Some(other_files) = args.get("otherFiles") else {
        return Ok(Vec::new());
    };
    let other_files = other_files
        .as_array()
        .ok_or_else(|| anyhow!("otherFiles must be an array"))?;
    let mut decoded = Vec::with_capacity(other_files.len());
    for file in other_files {
        let file = file
            .as_object()
            .ok_or_else(|| anyhow!("otherFiles entries must be objects"))?;
        let name = file
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("otherFiles.name is required"))?;
        safe_relative_path(name)?;
        if name == "SKILL.md" {
            return Err(anyhow!("otherFiles must not overwrite SKILL.md"));
        }
        let body = file
            .get("body")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("otherFiles.body is required"))?;
        let encoding = file
            .get("encoding")
            .and_then(Value::as_str)
            .unwrap_or("utf-8");
        let bytes = match encoding {
            "utf-8" => body.as_bytes().to_vec(),
            "base64" => decode_base64(body)
                .with_context(|| format!("Invalid base64 body for other file {name}"))?,
            _ => return Err(anyhow!("otherFiles.encoding must be 'utf-8' or 'base64'")),
        };
        if bytes.len() > 1024 * 1024 {
            return Err(anyhow!("skill file {name} exceeds maximum 1MB size"));
        }
        decoded.push((name.to_owned(), bytes));
    }
    Ok(decoded)
}

fn mcp_target_path(args: &Map<String, Value>, feature: &str) -> Result<PathBuf> {
    let raw = mcp_required_string(args, "targetPathFromCwd")?;
    safe_relative_path(&raw)?;
    let input = PathBuf::from(&raw);
    if feature == "skill" {
        let relative = if input.file_name().and_then(|value| value.to_str()) == Some("SKILL.md") {
            input
                .parent()
                .map(Path::to_path_buf)
                .ok_or_else(|| anyhow!("skill target path must name a skill directory"))?
        } else {
            input
        };
        let root = Path::new(".carabiner/skills");
        if relative != root && !relative.starts_with(root.join("")) {
            return Err(anyhow!(
                "skill target path must be inside .carabiner/skills"
            ));
        }
        let name = relative
            .strip_prefix(root)
            .ok()
            .and_then(|path| path.components().next())
            .and_then(|component| component.as_os_str().to_str())
            .ok_or_else(|| anyhow!("skill target path must name a skill directory"))?;
        safe_name(name)?;
        return Ok(relative.join("SKILL.md"));
    }
    let directory =
        mcp_markdown_dir(feature).ok_or_else(|| anyhow!("unknown MCP feature '{feature}'"))?;
    let filename = input
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow!("targetPathFromCwd must name a file"))?;
    if !filename.ends_with(".md") || filename.starts_with('.') {
        return Err(anyhow!("{feature} target path must name a Markdown file"));
    }
    Ok(PathBuf::from(directory).join(filename))
}

fn mcp_read_markdown(feature: &str, args: &Map<String, Value>) -> Result<Value> {
    let path = mcp_target_path(args, feature)?;
    let content =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let parsed = parse_frontmatter(&content, &path)?;
    if !parsed.has_frontmatter {
        return Err(anyhow!(
            "Missing frontmatter in {}. Carabiner files must begin with a YAML frontmatter block delimited by '---'.",
            path.display()
        ));
    }
    let parsed_data = Value::Object(parsed.data);
    let frontmatter = Value::Object(mcp_frontmatter(feature, Some(&parsed_data))?);
    if feature == "skill" {
        let skill_root = path
            .parent()
            .ok_or_else(|| anyhow!("skill target path must name a skill directory"))?;
        return Ok(json!({
            "relativeDirPathFromCwd": skill_root.to_string_lossy().replace('\\', "/"),
            "frontmatter": frontmatter,
            "body": parsed.body,
            "otherFiles": mcp_skill_other_files(skill_root)?,
        }));
    }
    Ok(json!({
        "relativePathFromCwd": path.to_string_lossy().replace('\\', "/"),
        "frontmatter": frontmatter,
        "body": parsed.body,
    }))
}

fn mcp_list_markdown(feature: &str) -> Result<Value> {
    let directory =
        mcp_markdown_dir(feature).ok_or_else(|| anyhow!("unknown MCP feature '{feature}'"))?;
    let root = PathBuf::from(directory);
    let mut items = Vec::new();
    for path in walk_files(&root) {
        if feature == "skill" {
            if path.file_name().and_then(|value| value.to_str()) != Some("SKILL.md")
                || path.parent().and_then(Path::parent) != Some(root.as_path())
            {
                continue;
            }
        } else if path.parent() != Some(root.as_path()) {
            continue;
        }
        let content = match fs::read_to_string(&path) {
            Ok(content) => content,
            Err(_) => continue,
        };
        let parsed = match parse_frontmatter(&content, &path) {
            Ok(parsed) if parsed.has_frontmatter => parsed,
            _ => continue,
        };
        let data = Value::Object(parsed.data);
        let frontmatter = match mcp_frontmatter(feature, Some(&data)) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let relative = path.to_string_lossy().replace('\\', "/");
        let item = if feature == "skill" {
            let relative_dir = path
                .parent()
                .map(|value| value.to_string_lossy().replace('\\', "/"))
                .unwrap_or(relative);
            json!({
                "relativeDirPathFromCwd": relative_dir,
                "frontmatter": frontmatter,
            })
        } else {
            json!({
                "relativePathFromCwd": relative,
                "frontmatter": frontmatter,
            })
        };
        items.push(item);
    }
    items.sort_by(|left, right| {
        let left_path = left
            .get("relativePathFromCwd")
            .or_else(|| left.get("relativeDirPathFromCwd"))
            .and_then(Value::as_str);
        let right_path = right
            .get("relativePathFromCwd")
            .or_else(|| right.get("relativeDirPathFromCwd"))
            .and_then(Value::as_str);
        left_path.cmp(&right_path)
    });
    let key = match feature {
        "rule" => "rules",
        "command" => "commands",
        "subagent" => "subagents",
        "skill" => "skills",
        "check" => "checks",
        _ => "items",
    };
    Ok(Value::Object(Map::from_iter([(
        key.into(),
        Value::Array(items),
    )])))
}

fn mcp_put_markdown(feature: &str, args: &Map<String, Value>) -> Result<Value> {
    let path = mcp_target_path(args, feature)?;
    let body = args
        .get("body")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("body is required for {feature} put operation"))?;
    let frontmatter = mcp_frontmatter(feature, args.get("frontmatter"))?;
    let content = stringify_frontmatter(body, &frontmatter)?;
    if content.len() > 1024 * 1024 {
        return Err(anyhow!("{feature} file exceeds maximum 1MB size"));
    }
    if feature == "skill" {
        let skill_root = path
            .parent()
            .ok_or_else(|| anyhow!("skill target path must name a directory"))?;
        let decoded_files = mcp_decode_skill_files(args)?;
        let estimated_size = serde_json::to_vec(&Value::Object(frontmatter.clone()))?.len()
            + body.len()
            + decoded_files
                .iter()
                .map(|(name, bytes)| name.len() + bytes.len())
                .sum::<usize>();
        if estimated_size > 1024 * 1024 {
            return Err(anyhow!(
                "skill size {estimated_size} bytes exceeds maximum 1MB"
            ));
        }
        mcp_skill_path_is_safe(skill_root, &path)?;
        write_text_raw(&path, &content, false)?;
        for (name, bytes) in decoded_files {
            let target = skill_root.join(name);
            mcp_skill_path_is_safe(skill_root, &target)?;
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            write_bytes(&target, &bytes, false)?;
        }
        return Ok(json!({
            "relativeDirPathFromCwd": skill_root.to_string_lossy().replace('\\', "/"),
            "frontmatter": frontmatter,
            "body": body,
            "otherFiles": mcp_skill_other_files(skill_root)?,
        }));
    }
    write_text_raw(&path, &content, false)?;
    Ok(json!({
        "relativePathFromCwd": path.to_string_lossy().replace('\\', "/"),
        "frontmatter": frontmatter,
        "body": body,
    }))
}

fn mcp_delete_markdown(feature: &str, args: &Map<String, Value>) -> Result<Value> {
    let path = mcp_target_path(args, feature)?;
    if feature == "skill" {
        let skill_root = path
            .parent()
            .ok_or_else(|| anyhow!("skill target path must name a directory"))?;
        if fs::symlink_metadata(skill_root)
            .map(|metadata| metadata.file_type().is_symlink())
            .unwrap_or(false)
        {
            return Err(anyhow!(
                "Refusing to delete through a symbolic link: {}",
                skill_root.display()
            ));
        }
        if skill_root.is_dir() {
            fs::remove_dir_all(skill_root)?;
        }
        return Ok(json!({
            "relativeDirPathFromCwd": skill_root.to_string_lossy().replace('\\', "/")
        }));
    }
    if path.is_file() {
        fs::remove_file(&path)?;
    }
    Ok(json!({"relativePathFromCwd": path.to_string_lossy().replace('\\', "/")}))
}

fn mcp_singleton_operation(feature: &str, operation: &str) -> Result<Value> {
    let path = mcp_singleton_path(feature)?;
    match operation {
        "get" => {
            let content =
                fs::read_to_string(&path).with_context(|| format!("failed to read {path}"))?;
            Ok(json!({"relativePathFromCwd": path, "content": content}))
        }
        "put" => Err(anyhow!("content is required for {feature} put operation")),
        "delete" => {
            if let Some(candidates) = mcp_feature_candidates(feature) {
                for candidate in candidates {
                    if Path::new(candidate).is_file() {
                        fs::remove_file(candidate)?;
                    }
                }
            }
            Ok(json!({"relativePathFromCwd": path}))
        }
        _ => Err(anyhow!(
            "Operation {operation} is not supported for feature {feature}"
        )),
    }
}

fn mcp_put_singleton(feature: &str, args: &Map<String, Value>) -> Result<Value> {
    let path = mcp_singleton_path(feature)?;
    let content = args
        .get("content")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("content is required for {feature} put operation"))?;
    if content.len() > 1024 * 1024 {
        return Err(anyhow!("{feature} file exceeds maximum 1MB size"));
    }
    if feature != "ignore" {
        let value = parse_jsonc(content)
            .with_context(|| format!("Invalid JSONC format in {feature} file ({path})"))?;
        if !value.is_object() {
            return Err(anyhow!("{feature} content must contain an object"));
        }
    }
    write_text_raw(Path::new(&path), content, false)?;
    Ok(json!({"relativePathFromCwd": path, "content": content}))
}

fn mcp_string_array(args: &Map<String, Value>, key: &str) -> Result<Option<Vec<String>>> {
    let Some(value) = args.get(key) else {
        return Ok(None);
    };
    let values = value
        .as_array()
        .ok_or_else(|| anyhow!("{key} must be an array"))?;
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| anyhow!("{key} entries must be strings"))
        })
        .collect::<Result<Vec<_>>>()
        .map(Some)
}
fn decode_base64(input: &str) -> Result<Vec<u8>> {
    let compact = input
        .chars()
        .filter(|value| !value.is_ascii_whitespace())
        .collect::<String>();
    let first_padding = compact.find('=').unwrap_or(compact.len());
    if compact[first_padding..].chars().any(|value| value != '=')
        || compact[first_padding..].len() > 2
    {
        return Err(anyhow!("invalid base64 content"));
    }
    let data = &compact[..first_padding];
    let padding_count = compact.len() - first_padding;
    if data.len() % 4 == 1
        || (padding_count > 0 && (data.len() + padding_count) % 4 != 0)
        || (padding_count == 0 && data.len() % 4 == 1)
    {
        return Err(anyhow!("invalid base64 content"));
    }
    let mut output = Vec::with_capacity(data.len() / 4 * 3);
    let mut buffer = 0u32;
    let mut bits = 0u8;
    for byte in data.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return Err(anyhow!("invalid base64 content")),
        } as u32;
        buffer = (buffer << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push((buffer >> bits) as u8);
            if bits == 0 {
                buffer = 0;
            } else {
                buffer &= (1 << bits) - 1;
            }
        }
    }
    if bits >= 6 {
        return Err(anyhow!("invalid base64 content"));
    }
    let canonical_input = data.replace('-', "+").replace('_', "/");
    if encode_base64(&output).trim_end_matches('=') != canonical_input {
        return Err(anyhow!("invalid base64 content"));
    }
    Ok(output)
}

fn mcp_bool(args: &Map<String, Value>, key: &str) -> Result<Option<bool>> {
    let Some(value) = args.get(key) else {
        return Ok(None);
    };
    value
        .as_bool()
        .map(Some)
        .ok_or_else(|| anyhow!("{key} must be a boolean"))
}

fn mcp_generate_options(value: Option<&Value>) -> Result<ConfigOptions> {
    let default_value = Value::Object(Map::new());
    let object = value
        .unwrap_or(&default_value)
        .as_object()
        .ok_or_else(|| anyhow!("generateOptions must be an object"))?;
    Ok(ConfigOptions {
        targets: mcp_string_array(object, "targets")?,
        features: mcp_string_array(object, "features")?,
        delete: mcp_bool(object, "delete")?,
        global: mcp_bool(object, "global")?,
        simulate_commands: mcp_bool(object, "simulateCommands")?,
        simulate_subagents: mcp_bool(object, "simulateSubagents")?,
        simulate_skills: mcp_bool(object, "simulateSkills")?,
        silent: Some(true),
        ..ConfigOptions::default()
    })
}

fn mcp_counts(result: &GenerateResult) -> Value {
    json!({
        "rulesCount": result.rules.count,
        "ignoreCount": result.ignore.count,
        "mcpCount": result.mcp.count,
        "commandsCount": result.commands.count,
        "subagentsCount": result.subagents.count,
        "skillsCount": result.skills.count,
        "hooksCount": result.hooks.count,
        "permissionsCount": result.permissions.count,
        "checksCount": result.checks.count,
        "activationCount": result.activation.count,
        "totalCount": result.total_files(),
    })
}

fn mcp_generate_operation(args: &Map<String, Value>) -> Result<Value> {
    let result = (|| -> Result<Value> {
        let options = mcp_generate_options(args.get("generateOptions"))?;
        let config = Config::resolve(&options)?;
        let result = generate(options)?;
        let targets = config.targets().join(", ");
        let features = config.features().join(", ");
        let message = if result.total_files() > 0 {
            format!(
                "Generated {} file(s) for targets [{targets}] and features [{features}].",
                result.total_files()
            )
        } else {
            format!("No files needed updating for targets [{targets}] and features [{features}]. 'generate' only writes files whose content changed, so a totalCount of 0 means the outputs are already up to date — this is a successful no-op, not a failure.")
        };
        Ok(json!({
            "success": true,
            "message": message,
            "result": mcp_counts(&result),
            "config": {
                "targets": config.targets(),
                "features": config.features(),
                "global": config.global(),
                "delete": config.delete(),
                "simulateCommands": config.simulate_commands(),
                "simulateSubagents": config.simulate_subagents(),
                "simulateSkills": config.simulate_skills(),
            }
        }))
    })();
    Ok(match result {
        Ok(value) => value,
        Err(error) => json!({"success": false, "error": error.to_string()}),
    })
}
fn mcp_import_operation(args: &Map<String, Value>) -> Result<Value> {
    let result = (|| -> Result<Value> {
        let import_options = args
            .get("importOptions")
            .and_then(Value::as_object)
            .ok_or_else(|| anyhow!("importOptions is required for import feature"))?;
        let target = import_options
            .get("target")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("target is required. Please specify a tool to import from."))?;
        if target.is_empty() {
            return Ok(json!({
                "success": false,
                "error": "target is required. Please specify a tool to import from."
            }));
        }
        if target_spec(target).is_none() {
            return Ok(json!({
                "success": false,
                "error": format!("Invalid target tool '{target}'.")
            }));
        }
        let config = ConfigOptions {
            targets: Some(vec![target.to_owned()]),
            features: mcp_string_array(import_options, "features")?,
            global: mcp_bool(import_options, "global")?,
            silent: Some(true),
            ..ConfigOptions::default()
        };
        let resolved = Config::resolve(&config)?;
        let result = import_from_tool(ImportOptions::from_config(config))?;
        Ok(json!({
            "success": true,
            "result": {
                "rulesCount": result.rules,
                "ignoreCount": result.ignore,
                "mcpCount": result.mcp,
                "commandsCount": result.commands,
                "subagentsCount": result.subagents,
                "skillsCount": result.skills,
                "hooksCount": result.hooks,
                "permissionsCount": result.permissions,
                "checksCount": result.checks,
                "totalCount": result.total_files(),
            },
            "config": {"target": target, "features": resolved.features(), "global": resolved.global()}
        }))
    })();
    Ok(match result {
        Ok(value) => value,
        Err(error) => json!({"success": false, "error": error.to_string()}),
    })
}

fn mcp_convert_operation(args: &Map<String, Value>) -> Result<Value> {
    let result = (|| -> Result<Value> {
        let options = args
            .get("convertOptions")
            .and_then(Value::as_object)
            .ok_or_else(|| anyhow!("convertOptions is required for convert feature"))?;
        let from = options.get("from").and_then(Value::as_str).ok_or_else(|| {
            anyhow!("from is required. Please specify a source tool to convert from.")
        })?;
        if from.is_empty() {
            return Ok(json!({
                "success": false,
                "error": "from is required. Please specify a source tool to convert from."
            }));
        }
        if target_spec(from).is_none() {
            return Ok(json!({
                "success": false,
                "error": format!("Invalid source tool '{from}'. Must be one of: {}", all_targets().join(", "))
            }));
        }
        let to_value = options.get("to").ok_or_else(|| {
            anyhow!("to is required and must not be empty. Please specify destination tools.")
        })?;
        let to_values = to_value
            .as_array()
            .ok_or_else(|| anyhow!("to must be an array"))?;
        if to_values.is_empty() {
            return Ok(json!({
                "success": false,
                "error": "to is required and must not be empty. Please specify destination tools."
            }));
        }
        let mut to = Vec::new();
        for value in to_values {
            let target = value
                .as_str()
                .ok_or_else(|| anyhow!("to entries must be strings"))?;
            if target_spec(target).is_none() {
                return Ok(json!({
                    "success": false,
                    "error": format!("Invalid destination tool '{target}'. Must be one of: {}", all_targets().join(", "))
                }));
            }
            if !to.iter().any(|existing| existing == target) {
                to.push(target.to_owned());
            }
        }
        if to.iter().any(|target| target == from) {
            return Ok(json!({
                "success": false,
                "error": format!("Destination tools must not include the source tool '{from}'. Converting a tool onto itself is likely a mistake and may cause lossy round-trips.")
            }));
        }
        if std::iter::once(from)
            .chain(to.iter().map(String::as_str))
            .any(|target| matches!(target, "antigravity-plugin" | "claudecode-plugin"))
        {
            return Ok(json!({
                "success": false,
                "error": "Plugin packaging target is not supported by convert"
            }));
        }
        let features = mcp_string_array(options, "features")?.unwrap_or_else(all_features);
        let global = mcp_bool(options, "global")?;
        let dry_run = mcp_bool(options, "dryRun")?.unwrap_or(false);
        let config = ConfigOptions {
            targets: Some(
                std::iter::once(from.to_owned())
                    .chain(to.iter().cloned())
                    .collect(),
            ),
            features: Some(features),
            global,
            dry_run: Some(dry_run),
            silent: Some(true),
            ..ConfigOptions::default()
        };
        let resolved = Config::resolve(&config)?;
        let result = convert_from_tool(ConvertOptions::from_config(
            config,
            from.to_owned(),
            to.clone(),
        ))?;
        Ok(json!({
            "success": true,
            "result": {
                "rulesCount": result.rules,
                "ignoreCount": result.ignore,
                "mcpCount": result.mcp,
                "commandsCount": result.commands,
                "subagentsCount": result.subagents,
                "skillsCount": result.skills,
                "hooksCount": result.hooks,
                "permissionsCount": result.permissions,
                "checksCount": result.checks,
                "totalCount": result.total_files(),
            },
            "config": {"from": from, "to": to, "features": resolved.features(), "global": resolved.global(), "dryRun": dry_run}
        }))
    })();
    Ok(match result {
        Ok(value) => value,
        Err(error) => json!({"success": false, "error": error.to_string()}),
    })
}

fn validate_mcp_request(args: &Map<String, Value>) -> Result<()> {
    let feature = mcp_required_string(args, "feature")?;
    let operation = mcp_required_string(args, "operation")?;
    let features = [
        "rule",
        "command",
        "subagent",
        "skill",
        "check",
        "ignore",
        "mcp",
        "permissions",
        "hooks",
        "generate",
        "import",
        "convert",
    ];
    if !features.contains(&feature.as_str()) {
        return Err(anyhow!(
            "Invalid feature '{feature}'. Expected one of: {}",
            features.join(", ")
        ));
    }
    let supported = match feature.as_str() {
        "rule" | "command" | "subagent" | "skill" | "check" => {
            ["list", "get", "put", "delete"].as_slice()
        }
        "ignore" | "mcp" | "permissions" | "hooks" => ["get", "put", "delete"].as_slice(),
        "generate" | "import" | "convert" => ["run"].as_slice(),
        _ => &[] as &[&str],
    };
    if !supported.contains(&operation.as_str()) {
        return Err(anyhow!(
            "Operation {operation} is not supported for feature {feature}. Supported operations: {}",
            supported.join(", ")
        ));
    }
    if matches!(operation.as_str(), "get" | "put" | "delete")
        && matches!(
            feature.as_str(),
            "rule" | "command" | "subagent" | "skill" | "check"
        )
        && args
            .get("targetPathFromCwd")
            .and_then(Value::as_str)
            .is_none()
    {
        return Err(anyhow!(
            "targetPathFromCwd is required for {feature} {operation} operation"
        ));
    }
    if operation == "put"
        && matches!(
            feature.as_str(),
            "rule" | "command" | "subagent" | "skill" | "check"
        )
        && args
            .get("body")
            .and_then(Value::as_str)
            .is_none_or(|value| value.is_empty())
    {
        return Err(anyhow!("body is required for {feature} put operation"));
    }
    if operation == "put"
        && matches!(feature.as_str(), "ignore" | "mcp" | "permissions" | "hooks")
        && args
            .get("content")
            .and_then(Value::as_str)
            .is_none_or(|value| value.is_empty())
    {
        return Err(anyhow!("content is required for {feature} put operation"));
    }
    Ok(())
}

fn mcp_execute_tool(args: &Value) -> Result<String> {
    let object = args
        .as_object()
        .ok_or_else(|| anyhow!("tool arguments must be an object"))?;
    validate_mcp_request(object)?;
    let feature = mcp_required_string(object, "feature")?;
    let operation = mcp_required_string(object, "operation")?;
    let result = match feature.as_str() {
        "rule" | "command" | "subagent" | "skill" | "check" => match operation.as_str() {
            "list" => mcp_list_markdown(&feature)?,
            "get" => mcp_read_markdown(&feature, object)?,
            "put" => mcp_put_markdown(&feature, object)?,
            "delete" => mcp_delete_markdown(&feature, object)?,
            _ => {
                return Err(anyhow!(
                    "Operation {operation} is not supported for feature {feature}"
                ))
            }
        },
        "ignore" | "mcp" | "permissions" | "hooks" => {
            if operation == "put" {
                mcp_put_singleton(&feature, object)?
            } else {
                mcp_singleton_operation(&feature, &operation)?
            }
        }
        "generate" if operation == "run" => mcp_generate_operation(object)?,
        "import" if operation == "run" => mcp_import_operation(object)?,
        "convert" if operation == "run" => mcp_convert_operation(object)?,
        _ => {
            return Err(anyhow!(
                "Operation {operation} is not supported for feature {feature}"
            ))
        }
    };
    Ok(serde_json::to_string_pretty(&result)?)
}

fn mcp_tool_definition() -> Value {
    json!({
        "name": "carabinerTool",
        "description": "Manage Carabiner files through a single MCP tool. Features: rule/command/subagent/skill/check support list/get/put/delete; ignore/mcp/permissions/hooks support get/put/delete only; generate supports run only; import supports run only; convert supports run only. Parameters: list requires no targetPathFromCwd (lists all items); get/delete require targetPathFromCwd; put requires targetPathFromCwd, frontmatter, and body (or content for ignore/mcp/permissions/hooks); generate/run uses generateOptions to configure generation; import/run uses importOptions to configure import; convert/run uses convertOptions to configure conversion. skill otherFiles entries accept an optional encoding (\"utf-8\" by default, \"base64\" for binary files) and are returned with the encoding they require.",
        "inputSchema": {
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "properties": {
                "feature": {
                    "type": "string",
                    "enum": ["rule", "command", "subagent", "skill", "check", "ignore", "mcp", "permissions", "hooks", "generate", "import", "convert"]
                },
                "operation": {
                    "type": "string",
                    "enum": ["list", "get", "put", "delete", "run"]
                },
                "targetPathFromCwd": {"type": "string"},
                "frontmatter": {},
                "body": {"type": "string"},
                "otherFiles": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string"},
                            "body": {"type": "string"},
                            "encoding": {"type": "string", "enum": ["utf-8", "base64"]}
                        },
                        "required": ["name", "body"],
                        "additionalProperties": false
                    }
                },
                "content": {"type": "string"},
                "generateOptions": {
                    "type": "object",
                    "properties": {
                        "targets": {"type": "array", "items": {"type": "string"}},
                        "features": {"type": "array", "items": {"type": "string"}},
                        "delete": {"type": "boolean"},
                        "global": {"type": "boolean"},
                        "simulateCommands": {"type": "boolean"},
                        "simulateSubagents": {"type": "boolean"},
                        "simulateSkills": {"type": "boolean"}
                    },
                    "additionalProperties": false
                },
                "importOptions": {
                    "type": "object",
                    "properties": {
                        "target": {"type": "string"},
                        "features": {"type": "array", "items": {"type": "string"}},
                        "global": {"type": "boolean"}
                    },
                    "required": ["target"],
                    "additionalProperties": false
                },
                "convertOptions": {
                    "type": "object",
                    "properties": {
                        "from": {"type": "string"},
                        "to": {"type": "array", "items": {"type": "string"}},
                        "features": {"type": "array", "items": {"type": "string"}},
                        "global": {"type": "boolean"},
                        "dryRun": {"type": "boolean"}
                    },
                    "required": ["from", "to"],
                    "additionalProperties": false
                }
            },
            "required": ["feature", "operation"],
            "additionalProperties": false
        }
    })
}

fn mcp_command() -> Result<Value> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    eprintln!("Carabiner MCP server started via stdio");
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let request: Value = match serde_json::from_str(&line) {
            Ok(request) => request,
            Err(error) => {
                writeln!(
                    stdout,
                    "{}",
                    serde_json::to_string(&json!({
                        "jsonrpc": "2.0",
                        "id": Value::Null,
                        "error": {"code": -32700, "message": error.to_string()}
                    }))?
                )?;
                stdout.flush()?;
                continue;
            }
        };
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        let response = match method {
            "initialize" => {
                let params = request.get("params").and_then(Value::as_object);
                let valid_params = params
                    .and_then(|params| params.get("protocolVersion"))
                    .and_then(Value::as_str)
                    .is_some()
                    && params
                        .and_then(|params| params.get("capabilities"))
                        .and_then(Value::as_object)
                        .is_some()
                    && params
                        .and_then(|params| params.get("clientInfo"))
                        .and_then(Value::as_object)
                        .is_some();
                if !valid_params {
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": {"code": -32603, "message": "Invalid initialize parameters"}
                    })
                } else {
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "protocolVersion": "2024-11-05",
                            "capabilities": {"tools": {}, "logging": {}, "completions": {}},
                            "serverInfo": {"name": "Carabiner MCP Server", "version": VERSION},
                            "instructions": "This server handles Carabiner files including rules, commands, MCP, ignore files, subagents and skills for any AI agents. It should be used when you need those files."
                        }
                    })
                }
            }
            "notifications/initialized" | "notifications/cancelled" => continue,
            "tools/list" => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {"tools": [mcp_tool_definition()]}
            }),
            "tools/call" => {
                let params = request.get("params").and_then(Value::as_object);
                let name = params
                    .and_then(|params| params.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if name != "carabinerTool" {
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": {"code": -32602, "message": format!("Unknown tool: {name}")}
                    })
                } else {
                    let default_arguments = Value::Object(Map::new());
                    let arguments = params
                        .and_then(|params| params.get("arguments"))
                        .unwrap_or(&default_arguments);
                    let validation_error = arguments.as_object().map_or_else(
                        || Some(anyhow!("arguments must be an object")),
                        |arguments| validate_mcp_request(arguments).err(),
                    );
                    if let Some(error) = validation_error {
                        json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "error": {
                                "code": -32602,
                                "message": format!(
                                    "MCP error -32602: Tool 'carabinerTool' parameter validation failed: {}. Please check the parameter types and values according to the tool's schema.",
                                    error
                                )
                            }
                        })
                    } else {
                        match mcp_execute_tool(arguments) {
                            Ok(text) => json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": {"content": [{"type": "text", "text": text}]}
                            }),
                            Err(error) => json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": {
                                    "isError": true,
                                    "content": [{"type": "text", "text": error.to_string()}]
                                }
                            }),
                        }
                    }
                }
            }
            "ping" => json!({"jsonrpc": "2.0", "id": id, "result": {}}),
            _ => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": format!("Method not found: {method}")}
            }),
        };
        writeln!(stdout, "{}", serde_json::to_string(&response)?)?;
        stdout.flush()?;
    }
    Ok(json!({"started": true}))
}
