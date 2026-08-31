use crate::config::{Config, ConfigOptions};
use crate::model::{
    CanonicalModel, Command, Feature, FeatureResult, FlatGenerateResult, FlatImportResult,
    GenerateResult, GeneratedFile, ImportResult, Rule, Skill, SkillFile, Subagent,
};
use crate::targets::{all_targets, target_spec, DataFormat, RuleMode, ScopePaths, TargetSpec};
use crate::util::{
    direct_dirs, get_bool, get_string, get_string_array, home_dir, indent_yaml_sequences,
    json_pretty, object_value, parse_frontmatter, parse_jsonc, relative_slash, safe_name,
    safe_relative_path, stringify_frontmatter, stringify_frontmatter_flat, walk_files,
    walk_files_following_links, write_bytes, write_text,
};
use anyhow::{anyhow, Context, Result};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
type AmpHandler = (String, Option<String>);
type AmpHandlerGroup = (String, Vec<AmpHandler>);
#[derive(Debug, Clone)]
struct JsHookHandler {
    command: String,
    matcher: Option<String>,
    tool_gate: Option<String>,
    property_gate: Option<String>,
}
type JsHookGroups = Vec<(String, Vec<JsHookHandler>)>;

pub type GenerateOptions = ConfigOptions;

#[derive(Debug, Clone, Default)]
pub struct ImportOptions {
    pub target: String,
    pub features: Option<Vec<String>>,
    pub config_path: Option<String>,
    pub cwd: Option<PathBuf>,
    pub global: Option<bool>,
    pub verbose: Option<bool>,
    pub silent: Option<bool>,
    pub output_root: Option<String>,
}

impl ImportOptions {
    pub fn from_config(config: ConfigOptions) -> Self {
        Self {
            target: config
                .targets
                .as_ref()
                .and_then(|targets| targets.first())
                .cloned()
                .unwrap_or_default(),
            features: config.features.clone(),
            config_path: config.config_path,
            cwd: config.cwd,
            global: config.global,
            verbose: config.verbose,
            silent: config.silent,
            output_root: config
                .output_roots
                .as_ref()
                .and_then(|roots| roots.first())
                .cloned(),
        }
    }

    pub fn to_config_options(&self) -> ConfigOptions {
        ConfigOptions {
            cwd: self.cwd.clone(),
            config_path: self.config_path.clone(),
            targets: (!self.target.is_empty()).then(|| vec![self.target.clone()]),
            features: self.features.clone(),
            output_roots: self.output_root.clone().map(|root| vec![root]),
            global: self.global,
            verbose: self.verbose,
            silent: self.silent,
            ..ConfigOptions::default()
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ConvertOptions {
    pub from: String,
    pub to: Vec<String>,
    pub features: Option<Vec<String>>,
    pub config_path: Option<String>,
    pub cwd: Option<PathBuf>,
    pub global: Option<bool>,
    pub dry_run: Option<bool>,
    pub verbose: Option<bool>,
    pub silent: Option<bool>,
}

impl ConvertOptions {
    pub fn from_config(config: ConfigOptions, from: impl Into<String>, to: Vec<String>) -> Self {
        Self {
            from: from.into(),
            to,
            features: config.features.clone(),
            config_path: config.config_path,
            cwd: config.cwd,
            global: config.global,
            dry_run: config.dry_run,
            verbose: config.verbose,
            silent: config.silent,
        }
    }

    pub fn to_config_options(&self) -> ConfigOptions {
        ConfigOptions {
            cwd: self.cwd.clone(),
            config_path: self.config_path.clone(),
            features: self.features.clone(),
            global: self.global,
            dry_run: self.dry_run,
            verbose: self.verbose,
            silent: self.silent,
            ..ConfigOptions::default()
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct InputRootInspection {
    pub existing: Vec<PathBuf>,
    pub missing: Vec<PathBuf>,
    pub message: Option<String>,
}

fn sanitize_input_root(path: &Path) -> String {
    path.to_string_lossy()
        .chars()
        .filter(|character| {
            let code = *character as u32;
            !character.is_control()
                && !matches!(
                    code,
                    0x200E | 0x200F | 0x2028 | 0x2029 | 0x202A..=0x202E | 0x2066..=0x2069
                )
        })
        .collect()
}

pub fn inspect_input_roots(input_roots: &[PathBuf]) -> InputRootInspection {
    let mut inspection = InputRootInspection::default();
    let mut non_directories = HashSet::new();
    let mut invalid_overlays = Vec::new();

    for (index, root) in input_roots.iter().enumerate() {
        if root.is_dir() {
            inspection.existing.push(root.clone());
        } else {
            inspection.missing.push(root.clone());
            if root.is_file() {
                non_directories.insert(root.clone());
                if index > 0 {
                    invalid_overlays.push(root);
                }
            }
        }
    }

    let primary = input_roots.first();
    inspection.message = if let Some(primary) = primary {
        if inspection.existing.iter().any(|root| root == primary) {
            invalid_overlays.first().map(|root| {
                format!(
                    "Configured optional input root '{}' exists but is not a directory.",
                    sanitize_input_root(root)
                )
            })
        } else {
            let default_root = std::env::current_dir()
                .unwrap_or_default()
                .join(".carabiner");
            if primary == &default_root && !non_directories.contains(primary) {
                Some(format!(
                    "Carabiner source directory '{}' does not exist. Run 'carabiner init' first.",
                    sanitize_input_root(primary)
                ))
            } else if non_directories.contains(primary) {
                Some(format!(
                    "Configured primary input root '{}' exists but is not a directory. Point your input root setting ('inputRoots', or the deprecated 'inputRoot') at a directory.",
                    sanitize_input_root(primary)
                ))
            } else {
                Some(format!(
                    "Configured primary input root '{}' does not exist. Create the directory or update your input root setting ('inputRoots', or the deprecated 'inputRoot').",
                    sanitize_input_root(primary)
                ))
            }
        }
    } else {
        None
    };
    inspection
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceKind {
    Rules,
    Commands,
    Subagents,
    Checks,
}

fn ensure_required_input_files(config: &Config, model: &CanonicalModel) -> Result<()> {
    let missing = [
        (Feature::Ignore, model.ignore.is_none()),
        (Feature::Mcp, model.mcp.is_none()),
        (Feature::Hooks, model.hooks.is_none()),
        (Feature::Permissions, model.permissions.is_none()),
    ];
    for (feature, is_missing) in missing {
        if !is_missing
            || !config.targets().iter().any(|target| {
                config
                    .features_for(target)
                    .iter()
                    .any(|value| value == feature.as_str())
                    && target_spec(target)
                        .is_some_and(|spec| spec.supports(feature, config.global(), true))
            })
        {
            continue;
        }
        let root = config
            .input_roots()
            .first()
            .cloned()
            .unwrap_or_else(|| config.cwd().join(".carabiner"));
        let relative = match feature {
            Feature::Ignore => ".carabiner/.aiignore",
            Feature::Mcp => ".carabiner/mcp.jsonc",
            Feature::Hooks => ".carabiner/hooks.jsonc",
            Feature::Permissions => ".carabiner/permissions.jsonc",
            _ => unreachable!(),
        };
        let path = match feature {
            Feature::Ignore => root.join(".aiignore"),
            Feature::Mcp => root.join("mcp.jsonc"),
            Feature::Hooks => root.join("hooks.jsonc"),
            Feature::Permissions => root.join("permissions.jsonc"),
            _ => unreachable!(),
        };
        let detail = match feature {
            Feature::Ignore | Feature::Mcp => format!(
                "Error: ENOENT: no such file or directory, open '{}'",
                path.display()
            ),
            Feature::Hooks => {
                "Error: No .carabiner/hooks.jsonc or .carabiner/hooks.json found.".into()
            }
            Feature::Permissions => {
                "Error: No .carabiner/permissions.jsonc or .carabiner/permissions.json found."
                    .into()
            }
            _ => unreachable!(),
        };
        return Err(anyhow!(
            "Failed to load {} file ({}): {}",
            match feature {
                Feature::Ignore => "carabiner ignore",
                Feature::Mcp => "a Carabiner MCP",
                Feature::Hooks => "Carabiner hooks",
                Feature::Permissions => "Carabiner permissions",
                _ => unreachable!(),
            },
            relative,
            detail
        ));
    }
    Ok(())
}

pub fn generate(options: GenerateOptions) -> Result<GenerateResult> {
    let config = Config::resolve(&options)?;
    ensure_input_roots(&config)?;
    let model = load_model(config.input_roots())?;
    ensure_required_input_files(&config, &model)?;
    generate_model(&config, &model)
}
pub fn export_canonical_to_tool_directory(
    target: &str,
    source_root: &Path,
    output_root: &Path,
    features: &[String],
) -> Result<GenerateResult> {
    let spec = target_spec(target).ok_or_else(|| anyhow!("Invalid tool target '{target}'"))?;
    let config = Config::resolve(&ConfigOptions {
        cwd: Some(output_root.to_path_buf()),
        targets: Some(vec![target.to_owned()]),
        features: Some(features.to_vec()),
        output_roots: Some(vec![output_root.to_string_lossy().into_owned()]),
        input_roots: Some(vec![source_root.to_string_lossy().into_owned()]),
        global: Some(false),
        silent: Some(true),
        ..ConfigOptions::default()
    })?;
    let model = load_model(&[source_root.to_path_buf()])?;
    let selected = config.features_for(target);
    let internal = render_and_apply(&config, &model, &spec, output_root, &selected)?;
    let mut result = GenerateResult::default();
    merge_feature_result(&mut result, internal);
    activate_hermes_project_plugins(&config, &mut result)?;
    result.has_diff = result.has_diff || result.total_files() > 0;
    Ok(result)
}

pub fn import_from_tool(options: ImportOptions) -> Result<ImportResult> {
    if options.target.trim().is_empty() {
        return Err(anyhow!(
            "target is required. Please specify a tool to import from."
        ));
    }
    let config = Config::resolve(&options.to_config_options())?;
    let target = config
        .targets()
        .first()
        .ok_or_else(|| anyhow!("No tools found in --targets"))?;
    if config.targets().len() != 1 {
        return Err(anyhow!("Only one tool can be imported at a time"));
    }
    let spec = target_spec(target).ok_or_else(|| anyhow!("Invalid tool target '{target}'"))?;
    let output_root = config
        .output_roots(Some(target))
        .first()
        .cloned()
        .unwrap_or_else(|| config.cwd().to_path_buf());
    let model = load_model_from_tool(&spec, &output_root, config.global())?;
    if target == "takt"
        && config
            .features_for(target)
            .iter()
            .any(|feature| feature == "skills")
        && !walk_files_following_links(&output_root.join(".takt/facets/knowledge")).is_empty()
    {
        return Err(anyhow!(
            "Importing existing TAKT facet files into carabiner is not supported: TAKT files are plain Markdown and the original skill metadata cannot be recovered."
        ));
    }
    let canonical_cwd = if config.global()
        && matches!(target.as_str(), "hermesagent" | "kimi-code")
        && (std::env::var_os("HERMES_HOME").is_some()
            || std::env::var_os("KIMI_CODE_HOME").is_some())
    {
        home_dir().unwrap_or_else(|_| output_root.clone())
    } else if config.global() {
        output_root.clone()
    } else {
        config.cwd().to_path_buf()
    };
    let features = config.features_for(target);
    if config.global() {
        write_canonical_model_global(
            &model,
            config.cwd(),
            &canonical_cwd,
            &features,
            target,
            config.preview(),
            target != "kimi-code",
        )
    } else {
        write_canonical_model(
            &model,
            &canonical_cwd,
            &features,
            config.preview(),
            target != "kimi-code",
        )
    }
}

pub fn convert_from_tool(options: ConvertOptions) -> Result<ImportResult> {
    if options.from.trim().is_empty() || options.to.is_empty() {
        return Err(anyhow!("from and to are required"));
    }
    let mut destinations = Vec::new();
    for target in &options.to {
        if !destinations.contains(target) {
            destinations.push(target.clone());
        }
    }
    if destinations.iter().any(|target| target == &options.from) {
        return Err(anyhow!(
            "Destination tools must not include the source tool '{}'.",
            options.from
        ));
    }
    if destinations
        .iter()
        .any(|target| matches!(target.as_str(), "antigravity-plugin" | "claudecode-plugin"))
        || matches!(
            options.from.as_str(),
            "antigravity-plugin" | "claudecode-plugin"
        )
    {
        return Err(anyhow!("Plugin packaging targets are not supported by convert. Use import and generate with an explicit plugin directory."));
    }
    let config = Config::resolve(&ConfigOptions {
        targets: Some({
            let mut values = vec![options.from.clone()];
            values.extend(destinations.clone());
            values
        }),
        ..options.to_config_options()
    })?;
    let source_spec = target_spec(&options.from)
        .ok_or_else(|| anyhow!("Invalid source tool '{}'.", options.from))?;
    let source_root = config
        .output_roots(Some(&options.from))
        .first()
        .cloned()
        .unwrap_or_else(|| config.cwd().to_path_buf());
    let model = load_model_from_tool(&source_spec, &source_root, config.global())?;
    let selected = config.features_for(&options.from);
    let mut result = ImportResult::default();
    for destination in &destinations {
        let spec = target_spec(destination)
            .ok_or_else(|| anyhow!("Invalid destination tool '{destination}'."))?;
        let output_roots = config.output_roots(Some(destination));
        for output_root in output_roots {
            let feature_result = render_and_apply(&config, &model, &spec, &output_root, &selected)?;
            result.rules += feature_result.rules.count;
            result.ignore += feature_result.ignore.count;
            result.mcp += feature_result.mcp.count;
            result.commands += feature_result.commands.count;
            result.subagents += feature_result.subagents.count;
            result.skills += feature_result.skills.count;
            result.hooks += feature_result.hooks.count;
            result.permissions += feature_result.permissions.count;
            result.checks += feature_result.checks.count;
        }
    }
    Ok(result)
}

pub fn generate_flat(options: GenerateOptions) -> Result<FlatGenerateResult> {
    generate(options).map(|result| result.flat())
}

pub fn import_from_tool_flat(options: ImportOptions) -> Result<FlatImportResult> {
    import_from_tool(options).map(|result| result.flat())
}

pub fn convert_from_tool_flat(options: ConvertOptions) -> Result<FlatImportResult> {
    convert_from_tool(options).map(|result| result.flat())
}

fn ensure_input_roots(config: &Config) -> Result<()> {
    if config.input_roots().is_empty() {
        return Err(anyhow!("input roots must not be empty"));
    }
    if let Some(message) = inspect_input_roots(config.input_roots()).message {
        return Err(anyhow!("{message}"));
    }
    Ok(())
}

fn load_model(input_roots: &[PathBuf]) -> Result<CanonicalModel> {
    let mut model = CanonicalModel::default();
    let mut rules_by_name: HashMap<String, Rule> = HashMap::new();
    let mut commands_by_name: HashMap<String, Command> = HashMap::new();
    let mut subagents_by_name: HashMap<String, Subagent> = HashMap::new();
    let mut checks_by_name: HashMap<String, Command> = HashMap::new();
    let mut skills_by_name: HashMap<String, Skill> = HashMap::new();

    for root in input_roots {
        if !root.is_dir() {
            continue;
        }
        for item in load_source_markdown(root, "rules", SourceKind::Rules)? {
            rules_by_name.insert(
                item.relative_path.to_lowercase(),
                Rule {
                    relative_path: item.relative_path,
                    frontmatter: item.frontmatter,
                    body: item.body,
                },
            );
        }
        for item in load_source_markdown(root, "commands", SourceKind::Commands)? {
            commands_by_name.insert(item.relative_path.to_lowercase(), item);
        }
        for item in load_source_markdown(root, "subagents", SourceKind::Subagents)? {
            subagents_by_name.insert(
                item.relative_path.to_lowercase(),
                Subagent {
                    relative_path: item.relative_path,
                    frontmatter: item.frontmatter,
                    body: item.body,
                },
            );
        }
        for item in load_source_markdown(root, "checks", SourceKind::Checks)? {
            checks_by_name.insert(item.relative_path.to_lowercase(), item);
        }
        for skill in load_source_skills(root)? {
            skills_by_name.insert(skill.name.to_lowercase(), skill);
        }
    }

    model.rules = sorted_values(rules_by_name);
    model.commands = sorted_values(commands_by_name);
    model.subagents = sorted_values(subagents_by_name);
    model.checks = sorted_values(checks_by_name);
    model.skills = sorted_values(skills_by_name);
    model.mcp = load_singleton(input_roots, &["mcp.jsonc", "mcp.json", ".mcp.json"], true)?;
    model.hooks = load_singleton(input_roots, &["hooks.jsonc", "hooks.json"], false)?;
    model.permissions = load_singleton(
        input_roots,
        &["permissions.jsonc", "permissions.json"],
        false,
    )?;
    model.ignore = load_ignore(input_roots);
    Ok(model)
}

fn sorted_values<T>(map: HashMap<String, T>) -> Vec<T> {
    let mut entries = map.into_iter().collect::<Vec<_>>();
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    entries.into_iter().map(|(_, value)| value).collect()
}

fn validate_source_frontmatter(frontmatter: &Map<String, Value>, path: &Path) -> Result<()> {
    if let Some(value) = frontmatter.get("targets") {
        let targets = value
            .as_array()
            .ok_or_else(|| anyhow!("'targets' in {} must be an array", path.display()))?;
        for target in targets {
            let target = target.as_str().ok_or_else(|| {
                anyhow!("'targets' entries in {} must be strings", path.display())
            })?;
            if target != "*" && !all_targets().iter().any(|known| known == target) {
                return Err(anyhow!("Unknown target '{target}' in {}", path.display()));
            }
        }
    }
    if let Some(value) = frontmatter.get("globs") {
        let globs = value
            .as_array()
            .ok_or_else(|| anyhow!("'globs' in {} must be an array", path.display()))?;
        if globs.iter().any(|glob| !glob.is_string()) {
            return Err(anyhow!(
                "'globs' entries in {} must be strings",
                path.display()
            ));
        }
    }

    Ok(())
}
fn valid_check_frontmatter(frontmatter: &Map<String, Value>) -> bool {
    frontmatter
        .get("severity")
        .map(|value| {
            value
                .as_str()
                .map(|severity| matches!(severity, "low" | "medium" | "high" | "critical"))
                .unwrap_or(false)
        })
        .unwrap_or(true)
}

fn load_source_markdown(root: &Path, subdir: &str, kind: SourceKind) -> Result<Vec<Command>> {
    let base = root.join(subdir);
    if !base.is_dir() {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    let mut paths = walk_files_following_links(&base)
        .into_iter()
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("md"))
        .collect::<Vec<_>>();
    paths.sort();
    for path in paths {
        let relative_path = relative_slash(&base, &path);
        if relative_path.split('/').any(|part| part == ".curated") {
            continue;
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let parsed = parse_frontmatter(&content, &path)?;
        if !parsed.has_frontmatter {
            return Err(anyhow!("Missing frontmatter in {}. Carabiner files must begin with a YAML frontmatter block delimited by '---'.", path.display()));
        }
        let mut frontmatter = parsed.data;
        frontmatter
            .entry("targets")
            .or_insert_with(|| Value::Array(vec![Value::String("*".into())]));
        if kind == SourceKind::Checks && !valid_check_frontmatter(&frontmatter) {
            continue;
        }
        validate_source_frontmatter(&frontmatter, &path)?;
        match kind {
            SourceKind::Rules => {
                frontmatter.entry("root").or_insert(Value::Bool(false));
                frontmatter.entry("localRoot").or_insert(Value::Bool(false));
            }
            SourceKind::Subagents => {
                if frontmatter
                    .get("name")
                    .and_then(Value::as_str)
                    .filter(|name| !name.is_empty())
                    .is_none()
                {
                    return Err(anyhow!(
                        "Missing required subagent name in {}",
                        path.display()
                    ));
                }
            }
            _ => {}
        }
        let item = Command {
            relative_path,
            frontmatter,
            body: parsed.body,
        };
        result.push(item);
    }
    // Local source files win over the curated tree, including nested paths.
    let curated = root.join(subdir).join(".curated");
    if curated.is_dir() {
        for path in walk_files_following_links(&curated)
            .into_iter()
            .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("md"))
        {
            let relative_path = relative_slash(&curated, &path);
            if result
                .iter()
                .any(|item| item.relative_path.eq_ignore_ascii_case(&relative_path))
            {
                continue;
            }
            let content = fs::read_to_string(&path)?;
            let parsed = parse_frontmatter(&content, &path)?;
            if !parsed.has_frontmatter {
                continue;
            }
            let mut frontmatter = parsed.data;
            frontmatter
                .entry("targets")
                .or_insert_with(|| Value::Array(vec![Value::String("*".into())]));
            if kind == SourceKind::Checks && !valid_check_frontmatter(&frontmatter) {
                continue;
            }
            validate_source_frontmatter(&frontmatter, &path)?;
            if kind == SourceKind::Rules {
                frontmatter.entry("root").or_insert(Value::Bool(false));
                frontmatter.entry("localRoot").or_insert(Value::Bool(false));
            }
            result.push(Command {
                relative_path,
                frontmatter,
                body: parsed.body,
            });
        }
    }
    Ok(result)
}

fn load_source_skills(root: &Path) -> Result<Vec<Skill>> {
    let base = root.join("skills");
    if !base.is_dir() {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    for parent in [base.clone(), base.join(".curated")] {
        if !parent.is_dir() {
            continue;
        }
        for dir in direct_dirs(&parent) {
            let name = dir
                .file_name()
                .and_then(|v| v.to_str())
                .unwrap_or_default()
                .to_owned();
            if name == ".curated"
                || result
                    .iter()
                    .any(|skill: &Skill| skill.name.eq_ignore_ascii_case(&name))
            {
                continue;
            }
            safe_name(&name)
                .with_context(|| format!("invalid skill directory {}", dir.display()))?;
            let main = dir.join("SKILL.md");
            if !main.is_file() {
                continue;
            }
            let content = fs::read_to_string(&main)?;
            let parsed = parse_frontmatter(&content, &main)?;
            if !parsed.has_frontmatter {
                return Err(anyhow!("Missing frontmatter in {}", main.display()));
            }
            let mut frontmatter = parsed.data;
            frontmatter
                .entry("name")
                .or_insert(Value::String(name.clone()));
            if frontmatter
                .get("description")
                .and_then(Value::as_str)
                .is_none()
            {
                return Err(anyhow!(
                    "Invalid skill {}: description must be a string",
                    main.display()
                ));
            }
            frontmatter
                .entry("targets")
                .or_insert_with(|| Value::Array(vec![Value::String("*".into())]));
            let mut other_files = Vec::new();
            for path in walk_files_following_links(&dir) {
                if path == main {
                    continue;
                }
                let rel = relative_slash(&dir, &path);
                if is_never_carried(&rel) {
                    continue;
                }
                other_files.push(SkillFile {
                    relative_path: rel,
                    content: fs::read(&path)?,
                });
            }
            result.push(Skill {
                name,
                frontmatter,
                body: parsed.body,
                other_files,
            });
        }
    }
    Ok(result)
}

fn is_never_carried(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.split('/').any(|part| {
        matches!(
            part,
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

fn load_singleton(
    input_roots: &[PathBuf],
    names: &[&str],
    merge_servers: bool,
) -> Result<Option<Value>> {
    let mut found = Vec::new();
    for root in input_roots {
        for name in names {
            let path = root.join(name);
            if path.is_file() {
                let value = parse_jsonc(&fs::read_to_string(&path)?)
                    .with_context(|| format!("failed to parse {}", path.display()))?;
                if merge_servers && value.get("mcpServers").and_then(Value::as_object).is_none() {
                    return Err(anyhow!(
                        "Invalid MCP source file '{}': 'mcpServers' must be an object.",
                        path.display()
                    ));
                }
                found.push(value);
                break;
            }
        }
    }
    let Some(first) = found.first().cloned() else {
        return Ok(None);
    };
    if merge_servers {
        let mut merged = first;
        for next in found.iter().skip(1) {
            merged = merge_mcp_values(&merged, next);
        }
        return Ok(Some(merged));
    }
    // Singleton overlays replace the complete file, matching hooks and
    // permissions' whole-file policy.
    Ok(found.last().cloned())
}

fn merge_mcp_values(base: &Value, overlay: &Value) -> Value {
    let mut merged = match base {
        Value::Object(map) => map.clone(),
        _ => Map::new(),
    };
    let overlay_map = match overlay {
        Value::Object(map) => map,
        _ => return overlay.clone(),
    };
    for (key, value) in overlay_map {
        if key == "mcpServers" || key.ends_with(".mcpServers") {
            let mut servers = match merged.get(key) {
                Some(Value::Object(map)) => map.clone(),
                _ => Map::new(),
            };
            if let Value::Object(next_servers) = value {
                for (name, server) in next_servers {
                    servers.insert(name.clone(), server.clone());
                }
            }
            merged.insert(key.clone(), Value::Object(servers));
        } else if let Value::Object(block) = value {
            let mut next = match merged.get(key) {
                Some(Value::Object(map)) => map.clone(),
                _ => Map::new(),
            };
            for (subkey, subvalue) in block {
                next.insert(subkey.clone(), subvalue.clone());
            }
            merged.insert(key.clone(), Value::Object(next));
        } else {
            merged.insert(key.clone(), value.clone());
        }
    }
    Value::Object(merged)
}

fn load_ignore(input_roots: &[PathBuf]) -> Option<String> {
    let mut result = None;
    for root in input_roots {
        let candidates = [
            root.join(".aiignore"),
            root.parent().unwrap_or(root).join(".carabinerignore"),
        ];
        for path in candidates {
            if path.is_file() {
                result = fs::read_to_string(path).ok();
                break;
            }
        }
    }
    result
}

fn generate_model(config: &Config, model: &CanonicalModel) -> Result<GenerateResult> {
    let mut result = GenerateResult::default();
    for target in config.targets() {
        let spec = target_spec(target).ok_or_else(|| anyhow!("Invalid tool target '{target}'"))?;
        let features = config.features_for(target);
        for output_root in config.output_roots(Some(target)) {
            let feature_result = render_and_apply(config, model, &spec, &output_root, &features)?;
            merge_feature_result(&mut result, feature_result);
        }
    }
    activate_hermes_project_plugins(config, &mut result)?;
    if config.targets().iter().any(|target| {
        config
            .features_for(target)
            .iter()
            .any(|feature| feature == "skills")
    }) {
        let source_output_root = config
            .input_roots()
            .last()
            .and_then(|root| root.parent())
            .unwrap_or(config.cwd());
        for target in config.targets() {
            let Some(spec) = target_spec(target) else {
                continue;
            };
            let features = config.features_for(target);
            if !features.iter().any(|feature| feature == "skills")
                || !spec.supports(Feature::Skills, config.global(), config.simulate_skills())
            {
                continue;
            }
            for _output_root in config.output_roots(Some(target)) {
                for skill in &model.skills {
                    result.skill_details.push(skill_detail_json(
                        skill,
                        source_output_root,
                        config.global(),
                    ));
                }
            }
        }
    }
    result.has_diff = result.has_diff || result.total_files() > 0 && config.preview();
    Ok(result)
}
fn activate_hermes_project_plugins(config: &Config, result: &mut GenerateResult) -> Result<()> {
    if config.global()
        || !config
            .targets()
            .iter()
            .any(|target| target == "hermesagent")
    {
        return Ok(());
    }
    let features = config.features_for("hermesagent");
    let descriptors = [
        ("ignore", "carabiner-ignore", Feature::Ignore),
        ("subagents", "carabiner-subagents", Feature::Subagents),
        ("checks", "carabiner-checks", Feature::Checks),
    ];
    let mut plugins = Vec::new();
    let output_roots = config.output_roots(Some("hermesagent"));
    for (feature, plugin, result_feature) in descriptors {
        if !features.iter().any(|enabled| enabled == feature) {
            continue;
        }
        let manifest = format!("{}/plugin.yaml", hermes_plugin_dir(false, feature));
        let changed = result
            .feature(result_feature)
            .paths
            .iter()
            .any(|path| path == &manifest);
        let exists = output_roots
            .iter()
            .any(|root| root.join(&manifest).is_file());
        if changed || exists {
            plugins.push(plugin);
        }
    }
    if plugins.is_empty() {
        return Ok(());
    }

    let configured_home = std::env::var_os("HERMES_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let home = if let Some(path) = configured_home {
        if path.is_absolute() {
            path
        } else {
            config.cwd().join(path)
        }
    } else {
        home_dir()?.join(".hermes")
    };
    let config_path = home.join("config.yaml");
    let existing_content = if config_path.is_file() {
        fs::read_to_string(&config_path)
            .with_context(|| format!("failed to read Hermes config {}", config_path.display()))?
    } else {
        String::new()
    };
    let existing = if existing_content.trim().is_empty() {
        Value::Object(Map::new())
    } else {
        read_structured_file(&config_path)?
    };
    let mut document = existing.as_object().cloned().unwrap_or_default();
    let mut plugin_config = document
        .remove("plugins")
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    let disabled = plugin_config
        .get("disabled")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();
    let conflicts = plugins
        .iter()
        .filter(|plugin| disabled.contains(*plugin))
        .copied()
        .collect::<Vec<_>>();
    if !conflicts.is_empty() {
        return Err(anyhow!(
            "Cannot activate Hermes project plugin(s) {} because {} explicitly lists them in plugins.disabled. Remove the conflicting entries or exclude those Carabiner features.",
            conflicts.join(", "),
            config_path.display()
        ));
    }
    let enabled = plugin_config
        .entry("enabled")
        .or_insert_with(|| Value::Array(Vec::new()));
    let enabled = enabled
        .as_array_mut()
        .ok_or_else(|| anyhow!("Hermes plugins.enabled must be an array"))?;
    for plugin in &plugins {
        if !enabled.iter().any(|value| value.as_str() == Some(*plugin)) {
            enabled.push(Value::String((*plugin).into()));
        }
    }
    document.insert("plugins".into(), Value::Object(plugin_config));
    let expected = serialize_json_or_yaml(&Value::Object(document), "config.yaml")?;
    let changed = if config.preview() {
        let normalized = crate::util::add_trailing_newline(&expected);
        crate::util::add_trailing_newline(&existing_content) != normalized
    } else {
        write_text(&config_path, &expected, false)?
    };
    if changed {
        result.activation.count += 1;
        let relative = if std::env::var_os("HERMES_HOME")
            .filter(|value| !value.is_empty())
            .is_some()
        {
            "config.yaml".to_owned()
        } else {
            ".hermes/config.yaml".to_owned()
        };
        result.activation.paths.push(relative);
        result.has_diff = true;
    }
    Ok(())
}

fn skill_detail_json(skill: &Skill, output_root: &Path, global: bool) -> Value {
    let other_files = skill
        .other_files
        .iter()
        .map(|file| {
            let data = file
                .content
                .iter()
                .copied()
                .map(|byte| Value::Number(serde_json::Number::from(byte)))
                .collect::<Vec<_>>();
            json!({
                "relativeFilePathToDirPath": file.relative_path,
                "fileBuffer": {"type": "Buffer", "data": data},
            })
        })
        .collect::<Vec<_>>();
    json!({
        "outputRoot": output_root,
        "relativeDirPath": ".carabiner/skills",
        "dirName": skill.name,
        "mainFile": {
            "name": "SKILL.md",
            "body": skill.body,
            "frontmatter": skill.frontmatter,
        },
        "otherFiles": other_files,
        "global": global,
    })
}

#[derive(Debug, Clone, Default)]
struct InternalResults {
    rules: FeatureResult,
    ignore: FeatureResult,
    mcp: FeatureResult,
    commands: FeatureResult,
    subagents: FeatureResult,
    skills: FeatureResult,
    hooks: FeatureResult,
    permissions: FeatureResult,
    checks: FeatureResult,
    activation: FeatureResult,
    has_diff: bool,
}

fn merge_feature_result(result: &mut GenerateResult, next: InternalResults) {
    result.rules.count += next.rules.count;
    result.rules.paths.extend(next.rules.paths);
    result.ignore.count += next.ignore.count;
    result.ignore.paths.extend(next.ignore.paths);
    result.mcp.count += next.mcp.count;
    result.mcp.paths.extend(next.mcp.paths);
    result.commands.count += next.commands.count;
    result.commands.paths.extend(next.commands.paths);
    result.subagents.count += next.subagents.count;
    result.subagents.paths.extend(next.subagents.paths);
    result.skills.count += next.skills.count;
    result.skills.paths.extend(next.skills.paths);
    result.hooks.count += next.hooks.count;
    result.hooks.paths.extend(next.hooks.paths);
    result.permissions.count += next.permissions.count;
    result.permissions.paths.extend(next.permissions.paths);
    result.checks.count += next.checks.count;
    result.checks.paths.extend(next.checks.paths);
    result.activation.count += next.activation.count;
    result.activation.paths.extend(next.activation.paths);
    result.has_diff |= next.has_diff;
}

fn check_file_owned_by_target(config: &Config, current: &TargetSpec, path: &str) -> bool {
    if !config.check() {
        return true;
    }
    let mut owner = None;
    let owner_targets = if config.config_exists() {
        config.config_file_targets()
    } else {
        config.targets()
    };
    for target in owner_targets {
        let Some(spec) = target_spec(target) else {
            continue;
        };
        let paths = spec.paths(config.global());
        if paths
            .root_rule
            .as_ref()
            .map(|root| root.path() == path)
            .unwrap_or(false)
            || (!config.global() && target == "rovodev" && path == "AGENTS.md")
        {
            owner = Some(target.as_str());
        }
    }
    owner.map(|target| target == current.name).unwrap_or(true)
}

fn render_and_apply(
    config: &Config,
    model: &CanonicalModel,
    spec: &TargetSpec,
    output_root: &Path,
    features: &[String],
) -> Result<InternalResults> {
    let global = config.global();
    let rule_options = config.feature_options(&spec.name, "rules");
    let mut result = InternalResults::default();
    let ignore_options = config.feature_options(&spec.name, "ignore");
    let enabled = |feature: Feature| features.iter().any(|value| value == feature.as_str());
    let apply =
        |result: &mut FeatureResult, files: Vec<GeneratedFile>, feature: Feature| -> Result<()> {
            let mut files = files;
            if feature == Feature::Rules && config.check() {
                files.retain(|file| check_file_owned_by_target(config, spec, &file.relative_path));
            }
            let generated = files
                .iter()
                .map(|file| file.relative_path.clone())
                .collect::<HashSet<_>>();
            *result = apply_feature_files(config, output_root, files, feature)?;
            delete_orphans(
                config,
                model,
                output_root,
                spec,
                feature,
                &generated,
                result,
            )?;
            Ok(())
        };
    let mut rule_files = if enabled(Feature::Rules) && spec.supports(Feature::Rules, global, true) {
        Some(render_rules(
            model,
            spec,
            global,
            config.simulate_commands(),
            config.simulate_subagents(),
            config.simulate_skills(),
            rule_options.as_ref(),
        )?)
    } else {
        None
    };
    if enabled(Feature::Ignore) && spec.supports(Feature::Ignore, global, true) {
        apply(
            &mut result.ignore,
            render_ignore(model, spec, global, output_root, ignore_options.as_ref())?,
            Feature::Ignore,
        )?;
    }
    if enabled(Feature::Mcp) && spec.supports(Feature::Mcp, global, true) {
        let mut mcp_files = render_mcp(model, spec, global, output_root)?;
        if let Some(rule_files) = rule_files.as_deref() {
            if let Some(instruction_file) =
                render_rule_instruction_config(spec, output_root, global, rule_files, &mcp_files)?
            {
                if let Some(existing) = mcp_files
                    .iter_mut()
                    .find(|file| file.relative_path == instruction_file.relative_path)
                {
                    *existing = instruction_file;
                } else {
                    mcp_files.push(instruction_file);
                }
            }
        }
        apply(&mut result.mcp, mcp_files, Feature::Mcp)?;
    }
    if enabled(Feature::Commands)
        && spec.supports(Feature::Commands, global, config.simulate_commands())
    {
        apply(
            &mut result.commands,
            render_commands(
                model,
                spec,
                global,
                output_root,
                config.flattened_command_naming(),
            )?,
            Feature::Commands,
        )?;
    }
    if enabled(Feature::Subagents)
        && spec.supports(Feature::Subagents, global, config.simulate_subagents())
    {
        apply(
            &mut result.subagents,
            render_subagents(model, spec, global, output_root, config.wildcard_targets())?,
            Feature::Subagents,
        )?;
    }
    if enabled(Feature::Skills) && spec.supports(Feature::Skills, global, config.simulate_skills())
    {
        apply(
            &mut result.skills,
            render_skills(model, spec, global)?,
            Feature::Skills,
        )?;
    }
    if enabled(Feature::Hooks) && spec.supports(Feature::Hooks, global, true) {
        apply(
            &mut result.hooks,
            render_hooks(model, spec, global, output_root, config.cwd())?,
            Feature::Hooks,
        )?;
    }
    if enabled(Feature::Permissions) && spec.supports(Feature::Permissions, global, true) {
        apply(
            &mut result.permissions,
            render_permissions(model, spec, global, output_root)?,
            Feature::Permissions,
        )?;
    }
    if enabled(Feature::Checks) && spec.supports(Feature::Checks, global, true) {
        apply(
            &mut result.checks,
            render_checks(model, spec, global, output_root)?,
            Feature::Checks,
        )?;
    }
    if enabled(Feature::Rules) && spec.supports(Feature::Rules, global, true) {
        apply(
            &mut result.rules,
            rule_files.take().unwrap_or_default(),
            Feature::Rules,
        )?;
    } else if enabled(Feature::Rules) {
        delete_orphans(
            config,
            model,
            output_root,
            spec,
            Feature::Rules,
            &HashSet::new(),
            &mut result.rules,
        )?;
    }
    result.has_diff = internal_has_diff(&result);
    Ok(result)
}

fn assert_output_path_safe(output_root: &Path, relative_path: &str) -> Result<()> {
    safe_relative_path(relative_path)?;
    if fs::symlink_metadata(output_root)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err(anyhow!(
            "refusing to write through symbolic link output root {}",
            output_root.display()
        ));
    }
    let target = output_root.join(relative_path);
    let mut current = target.as_path();
    loop {
        if fs::symlink_metadata(current)
            .map(|metadata| metadata.file_type().is_symlink())
            .unwrap_or(false)
        {
            return Err(anyhow!(
                "refusing to write through symbolic link {}",
                current.display()
            ));
        }
        if current == output_root {
            break;
        }
        let Some(parent) = current.parent() else {
            break;
        };
        if !parent.starts_with(output_root) {
            break;
        }
        current = parent;
    }
    Ok(())
}

fn generated_skill_directory(relative_path: &str) -> Option<String> {
    let components = relative_path.split('/').collect::<Vec<_>>();
    if let Some(index) = components
        .windows(2)
        .position(|window| matches!(window[0], "skills" | "skill"))
    {
        return Some(components[..=index + 1].join("/"));
    }
    components
        .windows(2)
        .position(|window| window[0] == "facets" && window[1] == "knowledge")
        .map(|index| components[..=index + 1].join("/"))
}

fn write_generated_file(
    output_root: &Path,
    file: &GeneratedFile,
    feature: Feature,
    dry_run: bool,
) -> Result<bool> {
    assert_output_path_safe(output_root, &file.relative_path)?;
    let path = output_root.join(&file.relative_path);
    let changed = if file.binary {
        write_bytes(&path, &file.content, dry_run)?
    } else {
        let text = String::from_utf8(file.content.clone()).unwrap_or_default();
        write_text(&path, &text, dry_run)?
    };
    #[cfg(unix)]
    if feature == Feature::Hooks
        && file.relative_path.contains(".clinerules/hooks/")
        && file
            .relative_path
            .rsplit('/')
            .next()
            .is_some_and(|name| !name.contains('.'))
        && !dry_run
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
    }
    #[cfg(not(unix))]
    let _ = feature;
    Ok(changed)
}

fn apply_feature_files(
    config: &Config,
    output_root: &Path,
    files: Vec<GeneratedFile>,
    feature: Feature,
) -> Result<FeatureResult> {
    let mut result = FeatureResult::default();
    if feature == Feature::Skills {
        let mut groups: Vec<(String, Vec<GeneratedFile>)> = Vec::new();
        for (index, file) in files.into_iter().enumerate() {
            let key = generated_skill_directory(&file.relative_path)
                .unwrap_or_else(|| format!("__skill_file_{index}"));
            if let Some((_, group)) = groups.iter_mut().find(|(name, _)| name == &key) {
                group.push(file);
            } else {
                groups.push((key, vec![file]));
            }
        }
        for (_, group) in groups {
            let changed = group.iter().try_fold(false, |changed, file| {
                Ok::<bool, anyhow::Error>(
                    changed || write_generated_file(output_root, file, feature, true)?,
                )
            })?;
            if !changed {
                continue;
            }
            if !config.preview() {
                for file in &group {
                    let _ = write_generated_file(output_root, file, feature, false)?;
                }
            }
            result.count += 1;
            result
                .paths
                .extend(group.into_iter().map(|file| file.relative_path));
        }
        return Ok(result);
    }
    for file in files {
        if write_generated_file(output_root, &file, feature, config.preview())? {
            result.count += 1;
            result.paths.push(file.relative_path);
        }
    }
    Ok(result)
}

fn canonical_frontmatter_for_tool(
    source: &Map<String, Value>,
    target: &str,
    feature: Feature,
) -> Map<String, Value> {
    let mut result = Map::new();
    let generic_keys: &[&str] = match feature {
        Feature::Commands | Feature::Skills => &["description"],
        Feature::Subagents => &["name", "description"],
        _ => &[],
    };
    for key in generic_keys {
        if let Some(value) = source.get(*key) {
            result.insert((*key).to_owned(), value.clone());
        }
    }
    if let Some(Value::Object(tool_fields)) = source.get(target) {
        for (key, value) in tool_fields {
            result.insert(key.clone(), value.clone());
        }
    }
    result
}
fn read_structured_or(path: &Path, default: Value) -> Result<Value> {
    if path.is_file() {
        read_structured_file(path)
    } else {
        Ok(default)
    }
}

fn render_rule_instruction_config(
    spec: &TargetSpec,
    output_root: &Path,
    global: bool,
    files: &[GeneratedFile],
    generated_shared_files: &[GeneratedFile],
) -> Result<Option<GeneratedFile>> {
    if spec.name != "opencode" && (spec.name != "kilo" || global) {
        return Ok(None);
    }
    let raw_path = spec
        .paths(global)
        .mcp
        .as_ref()
        .ok_or_else(|| anyhow!("{} has no shared config path", spec.name))?;
    let path = resolve_json_variant(output_root, raw_path);
    let output_path = output_root.join(path.path());
    let existing = if let Some(file) = generated_shared_files
        .iter()
        .find(|file| file.relative_path == path.path())
    {
        serde_json::from_slice(&file.content).unwrap_or(Value::Object(Map::new()))
    } else {
        read_structured_or(&output_path, Value::Object(Map::new()))?
    };
    if files.is_empty()
        && !output_path.exists()
        && !generated_shared_files
            .iter()
            .any(|file| file.relative_path == path.path())
    {
        return Ok(None);
    }
    let mut config = existing.as_object().cloned().unwrap_or_default();
    let existing_instructions = config
        .get("instructions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let managed_prefixes = if spec.name == "kilo" {
        vec![".kilo/rules/".to_owned()]
    } else if global {
        vec![
            "memories/".to_owned(),
            ".config/opencode/memories/".to_owned(),
        ]
    } else {
        vec![".opencode/memories/".to_owned()]
    };
    let mut instructions = existing_instructions
        .into_iter()
        .filter_map(|value| value.as_str().map(ToOwned::to_owned))
        .filter(|value| {
            let normalized = value.trim_start_matches("./");
            !managed_prefixes
                .iter()
                .any(|prefix| normalized.starts_with(prefix))
        })
        .collect::<Vec<_>>();
    for file in files {
        let Some(directory) = spec.paths(global).nonroot_rule_dir.as_deref() else {
            continue;
        };
        let prefix = format!("{directory}/");
        if !file.relative_path.starts_with(&prefix) {
            continue;
        }
        let instruction = if global && spec.name == "opencode" {
            file.relative_path
                .strip_prefix(".config/opencode/")
                .unwrap_or(&file.relative_path)
                .to_owned()
        } else {
            file.relative_path.clone()
        };
        instructions.push(instruction);
    }
    instructions.sort();
    instructions.dedup();
    if instructions.is_empty() {
        config.remove("instructions");
    } else {
        config.insert(
            "instructions".into(),
            Value::Array(instructions.into_iter().map(Value::String).collect()),
        );
    }
    Ok(Some(GeneratedFile::text(
        path.path(),
        serialize_json_or_yaml(&Value::Object(config), &path.file)?,
        Feature::Rules,
    )))
}

#[allow(clippy::too_many_arguments)]
fn render_rules(
    model: &CanonicalModel,
    spec: &TargetSpec,
    global: bool,
    simulate_commands: bool,
    simulate_subagents: bool,
    simulate_skills: bool,
    rule_options: Option<&Map<String, Value>>,
) -> Result<Vec<GeneratedFile>> {
    let paths = spec.paths(global);
    let include_local_root = match rule_options.and_then(|options| options.get("includeLocalRoot"))
    {
        None => true,
        Some(Value::Bool(value)) => *value,
        Some(_) => {
            return Err(anyhow!(
                "Invalid options for rules feature: 'includeLocalRoot' must be a boolean."
            ))
        }
    };
    let explicit_references = match rule_options.and_then(|options| options.get("ruleDiscoveryMode")) {
        None => None,
        Some(Value::String(value)) if value == "none" => Some(false),
        Some(Value::String(value)) if value == "explicit" => Some(true),
        Some(_) => return Err(anyhow!("Invalid options for rules feature: 'ruleDiscoveryMode' must be either \"none\" or \"explicit\".")),
    };
    let rules = model
        .rules
        .iter()
        .filter(|rule| rule.targeted_at(&spec.name))
        .collect::<Vec<_>>();
    if rules.is_empty() {
        return Ok(Vec::new());
    }
    let is_pi_append = |rule: &&Rule| {
        spec.name == "pi"
            && !rule.root()
            && rule
                .frontmatter
                .get("pi")
                .and_then(object_value)
                .and_then(|fields| get_string(fields, "systemPrompt"))
                .as_deref()
                == Some("append")
    };
    let append_rules = rules
        .iter()
        .filter(|rule| is_pi_append(rule))
        .copied()
        .collect::<Vec<_>>();
    let normal = rules
        .iter()
        .filter(|rule| !rule.local_root() && !is_pi_append(rule))
        .copied()
        .collect::<Vec<_>>();
    let normal = if global && spec.name == "claudecode-legacy" {
        normal.into_iter().filter(|rule| rule.root()).collect()
    } else {
        normal
    };
    let normal = if spec.name == "zed" {
        normal.into_iter().filter(|rule| rule.root()).collect()
    } else {
        normal
    };
    let local = if include_local_root {
        rules.iter().find(|rule| rule.local_root()).copied()
    } else {
        None
    };
    let root_path = paths.root_rule.as_ref().map(|path| {
        let override_root = spec.name == "pi"
            && normal.iter().any(|rule| {
                rule.root()
                    && rule
                        .frontmatter
                        .get("pi")
                        .and_then(object_value)
                        .and_then(|fields| get_string(fields, "contextFile"))
                        .as_deref()
                        == Some("override")
            });
        if override_root {
            if path.dir == "." {
                "AGENTS.override.md".into()
            } else {
                format!("{}/AGENTS.override.md", path.dir.trim_end_matches('/'))
            }
        } else {
            path.path()
        }
    });
    let references_enabled = explicit_references.unwrap_or_else(|| requires_references(&spec.name));
    let append_local_body = |body: String, has_root: bool| {
        if !global && paths.local_rule.is_none() && has_root {
            if let Some(local) = local {
                let local_body = local.body.trim();
                if !local_body.is_empty() {
                    return format!("{body}\n\n{local_body}");
                }
            }
        }
        body
    };
    let mut output = Vec::new();
    if !append_rules.is_empty() {
        output.push(GeneratedFile::text(
            pi_append_path(global),
            join_bodies(append_rules.iter().map(|rule| rule.body.as_str())),
            Feature::Rules,
        ));
    }

    if let (Some(nonroot_rule_dir), Some(_root_rule)) =
        (paths.nonroot_rule_dir.as_ref(), paths.root_rule.as_ref())
    {
        let root_rules = normal
            .iter()
            .filter(|rule| rule.root())
            .copied()
            .collect::<Vec<_>>();
        let nonroots = normal
            .iter()
            .filter(|rule| !rule.root())
            .copied()
            .collect::<Vec<_>>();
        if !root_rules.is_empty() {
            let root_body = join_bodies(root_rules.iter().map(|rule| rule.body.as_str()));
            let mut references = String::new();
            if references_enabled {
                references = if spec.name == "claudecode-legacy" {
                    legacy_references(&nonroots, paths)
                } else {
                    toon_references(&nonroots, paths, &spec.name)
                };
            }
            let reference_prefix = if references.is_empty() {
                String::new()
            } else if spec.name == "claudecode-legacy" {
                format!("Please also reference the following rules as needed:\n\n{references}\n\n")
            } else {
                format!("Please also reference the following rules as needed. The list below is provided in TOON format, and `@` stands for the project root directory.\n\n{references}\n\n")
            };
            let conventions = simulated_conventions(
                model,
                &spec.name,
                global,
                simulate_commands,
                simulate_subagents,
                simulate_skills,
            );
            let body =
                append_local_body(format!("{reference_prefix}{conventions}{root_body}"), true);
            output.push(GeneratedFile::text(
                root_path
                    .as_ref()
                    .expect("root_path is Some when paths.root_rule is Some")
                    .clone(),
                body.clone(),
                Feature::Rules,
            ));
            if spec.name == "rovodev" && !global {
                output.push(GeneratedFile::text("AGENTS.md", body, Feature::Rules));
            }
        }
        for rule in nonroots {
            if matches!(
                spec.name.as_str(),
                "agentsmd" | "kiro" | "kiro-cli" | "kiro-ide"
            ) {
                if let Some(subproject) = rule
                    .frontmatter
                    .get("agentsmd")
                    .and_then(object_value)
                    .and_then(|fields| get_string(fields, "subprojectPath"))
                {
                    safe_relative_path(&format!("{subproject}/AGENTS.md"))?;
                    output.push(GeneratedFile::text(
                        format!("{subproject}/AGENTS.md"),
                        render_rule_content(rule, &spec.name, false)?,
                        Feature::Rules,
                    ));
                    continue;
                }
            }
            if spec.name == "rovodev"
                && matches!(
                    rule.relative_path.to_ascii_lowercase().as_str(),
                    "agents.md" | "agents.local.md"
                )
            {
                return Err(anyhow!(
                    "Reserved Rovodev memory basename in modular rule path: {}",
                    rule.relative_path
                ));
            }
            let relative = if matches!(spec.name.as_str(), "antigravity-ide" | "antigravity-plugin")
            {
                kebab_filename(&rule.relative_path)
            } else {
                rule_output_name(&rule.relative_path, &paths.rule_ext)
            };
            let mut output_dir = nonroot_rule_dir.clone();
            if matches!(spec.name.as_str(), "roo" | "zoocode") {
                if let Some(mode) = rule
                    .frontmatter
                    .get("roo")
                    .and_then(object_value)
                    .and_then(|map| get_string(map, "mode"))
                {
                    if !mode.is_empty()
                        && mode
                            .chars()
                            .all(|character| character.is_ascii_alphanumeric() || character == '-')
                    {
                        output_dir = format!(".roo/rules-{mode}");
                    }
                }
            }
            safe_relative_path(&format!("{output_dir}/{relative}"))?;
            let path = format!("{output_dir}/{relative}");
            let body = render_rule_content(rule, &spec.name, false)?;
            output.push(GeneratedFile::text(path, body, Feature::Rules));
        }
    } else if paths.rule_mode == RuleMode::Fold && paths.root_rule.is_some() {
        let is_nested = |rule: &&Rule| {
            rule.frontmatter
                .get("agentsmd")
                .and_then(object_value)
                .and_then(|map| get_string(map, "subprojectPath"))
                .is_some()
        };
        let body = join_bodies(
            normal
                .iter()
                .filter(|rule| !is_nested(rule))
                .map(|rule| rule.body.as_str()),
        );
        let root_body = body;
        let referenced = normal
            .iter()
            .filter(|rule| !rule.root() && !is_nested(rule))
            .copied()
            .collect::<Vec<_>>();
        let references = if references_enabled {
            if spec.name == "claudecode-legacy" {
                legacy_references(&referenced, paths)
            } else {
                toon_references(&referenced, paths, &spec.name)
            }
        } else {
            String::new()
        };
        let prefix = if references.is_empty() {
            String::new()
        } else if spec.name == "claudecode-legacy" {
            format!("Please also reference the following rules as needed:\n\n{references}\n\n")
        } else {
            format!("Please also reference the following rules as needed. The list below is provided in TOON format, and `@` stands for the project root directory.\n\n{references}\n\n")
        };
        let content = append_local_body(
            format!("{prefix}{root_body}"),
            normal.iter().any(|rule| rule.root()),
        );
        if !content.is_empty() {
            output.push(GeneratedFile::text(
                root_path
                    .as_ref()
                    .expect("root_path is Some when paths.root_rule is Some")
                    .clone(),
                content,
                Feature::Rules,
            ));
        }
        if spec.name == "reasonix" {
            for rule in normal.iter().filter(|rule| is_nested(rule)) {
                let Some(subproject) = rule
                    .frontmatter
                    .get("agentsmd")
                    .and_then(object_value)
                    .and_then(|map| get_string(map, "subprojectPath"))
                else {
                    continue;
                };
                safe_relative_path(&format!("{subproject}/REASONIX.md"))?;
                output.push(GeneratedFile::text(
                    format!("{subproject}/REASONIX.md"),
                    rule.body.trim().to_owned(),
                    Feature::Rules,
                ));
            }
        }
    } else if let Some(nonroot_rule_dir) = paths.nonroot_rule_dir.as_ref() {
        // Cursor and similar rootless formats render every canonical rule as a
        // modular file; root metadata is represented by the tool file itself.
        for rule in normal {
            if spec.name == "takt" {
                let (path, body) = render_takt_rule(rule)?;
                output.push(GeneratedFile::text(path, body, Feature::Rules));
                continue;
            }
            let relative = rule_output_name(&rule.relative_path, &paths.rule_ext);
            let path = format!("{nonroot_rule_dir}/{relative}");
            let content = if spec.name == "augmentcode" && global {
                rule.body.trim().to_owned()
            } else {
                render_rule_content(rule, &spec.name, false)?
            };
            output.push(GeneratedFile::text(path, content, Feature::Rules));
        }
    } else if paths.root_rule.is_some() {
        let body = append_local_body(
            join_bodies(normal.iter().map(|rule| rule.body.as_str())),
            normal.iter().any(|rule| rule.root()),
        );
        if !body.is_empty() {
            output.push(GeneratedFile::text(
                root_path
                    .as_ref()
                    .expect("root_path is Some when paths.root_rule is Some")
                    .clone(),
                body,
                Feature::Rules,
            ));
        }
    }

    if !global {
        if let (Some(local_rule), Some(local)) = (&paths.local_rule, local) {
            output.push(GeneratedFile::text(
                local_rule.path(),
                local.body.clone(),
                Feature::Rules,
            ));
        }
    }
    Ok(output)
}

fn pi_append_path(global: bool) -> String {
    if global {
        ".pi/agent/APPEND_SYSTEM.md".into()
    } else {
        ".pi/APPEND_SYSTEM.md".into()
    }
}

fn pi_override_path(global: bool) -> String {
    if global {
        ".pi/agent/AGENTS.override.md".into()
    } else {
        "AGENTS.override.md".into()
    }
}

fn rule_output_name(source: &str, extension: &str) -> String {
    let mut name = source.replace('\\', "/");
    if extension == "mdc" {
        if name.ends_with(".md") {
            name.truncate(name.len() - 3);
        }
        name.push_str(".mdc");
    } else if extension != "md" {
        if name.ends_with(".md") {
            name.truncate(name.len() - 3);
        }
        name.push_str(extension);
    }
    name
}
fn kebab_filename(source: &str) -> String {
    let stem = Path::new(source)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(source);
    let mut output = String::new();
    for character in stem.chars() {
        if character.is_ascii_alphanumeric() {
            output.push(character.to_ascii_lowercase());
        } else if !output.ends_with('-') {
            output.push('-');
        }
    }
    format!("{}.md", output.trim_matches('-'))
}

fn join_bodies<'a>(bodies: impl Iterator<Item = &'a str>) -> String {
    bodies
        .filter(|body| !body.trim().is_empty())
        .map(|body| body.trim().to_owned())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn requires_references(target: &str) -> bool {
    matches!(
        target,
        "agentsmd"
            | "amp"
            | "antigravity-cli"
            | "claudecode-legacy"
            | "factorydroid"
            | "opencode"
            | "rovodev"
            | "warp"
            | "kiro"
            | "kiro-cli"
            | "kiro-ide"
    )
}

fn toon_scalar(value: &Value) -> String {
    match value {
        Value::String(text) => {
            let safe = !text.is_empty()
                && !text.chars().any(|c| c == ',' || c == '\n' || c == '\r')
                && !text.contains(": ")
                && !text.starts_with(['-', '[', '{', '"', '\'']);
            if safe {
                text.clone()
            } else {
                serde_json::to_string(text).unwrap_or_else(|_| "\"\"".into())
            }
        }
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::Null => "null".into(),
        _ => serde_json::to_string(value).unwrap_or_else(|_| "null".into()),
    }
}

fn toon_key(value: &str) -> String {
    if value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        value.into()
    } else {
        serde_json::to_string(value).unwrap_or_else(|_| "\"\"".into())
    }
}

fn toon_encode_object_array(key: &str, entries: &[Map<String, Value>]) -> String {
    if entries.is_empty() {
        return String::new();
    }
    let keys = entries[0].keys().cloned().collect::<Vec<_>>();
    let tabular = entries.iter().all(|entry| {
        entry.len() == keys.len()
            && keys.iter().all(|key| {
                entry
                    .get(key)
                    .map(|value| {
                        matches!(
                            value,
                            Value::String(_) | Value::Bool(_) | Value::Number(_) | Value::Null
                        )
                    })
                    .unwrap_or(false)
            })
    });
    if tabular {
        let mut out = format!(
            "{}[{}]{{{}}}:\n",
            key,
            entries.len(),
            keys.iter()
                .map(|key| toon_key(key))
                .collect::<Vec<_>>()
                .join(",")
        );
        for entry in entries {
            out.push_str("  ");
            out.push_str(
                &keys
                    .iter()
                    .map(|key| toon_scalar(entry.get(key).unwrap_or(&Value::Null)))
                    .collect::<Vec<_>>()
                    .join(","),
            );
            out.push('\n');
        }
        return out.trim_end_matches('\n').into();
    }
    let mut out = format!("{}[{}]:\n", key, entries.len());
    for entry in entries {
        let mut first = true;
        for (field, value) in entry {
            if first {
                out.push_str("  - ");
                first = false;
            } else {
                out.push_str("\n    ");
            }
            out.push_str(&toon_key(field));
            match value {
                Value::Array(values) if values.is_empty() => out.push_str(": []"),
                Value::Array(values) => {
                    out.push_str(&format!("[{}]: ", values.len()));
                    out.push_str(&values.iter().map(toon_scalar).collect::<Vec<_>>().join(","));
                }
                _ => {
                    out.push_str(": ");
                    out.push_str(&toon_scalar(value));
                }
            }
        }
        out.push('\n');
    }
    out.trim_end_matches('\n').into()
}

fn toon_references(rules: &[&Rule], paths: &ScopePaths, target: &str) -> String {
    let entries = rules
        .iter()
        .filter(|rule| !rule.root())
        .map(|rule| {
            let mut entry = Map::new();
            let relative = rule_output_name(&rule.relative_path, &paths.rule_ext);
            let path = if target == "agentsmd" {
                rule.frontmatter
                    .get("agentsmd")
                    .and_then(object_value)
                    .and_then(|fields| get_string(fields, "subprojectPath"))
                    .map(|subproject| format!("@{subproject}/AGENTS.md"))
                    .unwrap_or_else(|| {
                        format!(
                            "@{}",
                            paths
                                .nonroot_rule_dir
                                .as_ref()
                                .map(|dir| format!("{dir}/{relative}"))
                                .unwrap_or(relative)
                        )
                    })
            } else {
                format!(
                    "@{}",
                    paths
                        .nonroot_rule_dir
                        .as_ref()
                        .map(|dir| format!("{dir}/{relative}"))
                        .unwrap_or(relative)
                )
            };
            entry.insert("path".into(), Value::String(path));
            if let Some(description) = rule.description().filter(|value| !value.is_empty()) {
                entry.insert("description".into(), Value::String(description.into()));
            }
            let globs = rule.globs();
            if !globs.is_empty() {
                entry.insert(
                    "applyTo".into(),
                    Value::Array(globs.into_iter().map(Value::String).collect()),
                );
            }
            entry
        })
        .collect::<Vec<_>>();
    toon_encode_object_array("rules", &entries)
}
fn legacy_references(rules: &[&Rule], paths: &ScopePaths) -> String {
    rules
        .iter()
        .filter(|rule| !rule.root())
        .map(|rule| {
            let relative = rule_output_name(&rule.relative_path, &paths.rule_ext);
            let path = paths
                .nonroot_rule_dir
                .as_ref()
                .map(|dir| format!("{dir}/{relative}"))
                .unwrap_or(relative);
            let description = rule.description().unwrap_or("").replace('"', "\\\"");
            let globs = rule.globs().join(",");
            format!("@{path} description: \"{description}\" applyTo: \"{globs}\"")
        })
        .collect::<Vec<_>>()
        .join("\n")
}
fn simulated_conventions(
    model: &CanonicalModel,
    target: &str,
    global: bool,
    simulate_commands: bool,
    simulate_subagents: bool,
    simulate_skills: bool,
) -> String {
    let always_include = matches!(target, "qwencode" | "rovodev");
    if target == "agentsmd" {
        if global {
            return String::new();
        }
    } else if !always_include {
        return String::new();
    }
    let mut sections = vec!["# Additional Conventions Beyond the Built-in Functions\n\nAs this project's AI coding tool, you must follow the additional conventions below, in addition to the built-in functions.".to_owned()];
    if simulate_commands {
        sections.push("## Simulated Custom Slash Commands\n\nCustom slash commands allow you to define frequently-used prompts as Markdown files that you can execute.\n\n### Syntax\n\nUsers can use following syntax to invoke a custom command.\n\n```txt\ns/<command> [arguments]\n```\n\nThis syntax employs a double slash (`s/`) to prevent conflicts with built-in slash commands.\nThe `s` in `s/` stands for *simulate*. Because custom slash commands are not built-in, this syntax provides a pseudo way to invoke them.\n\nWhen users call a custom command, you have to look for the markdown file, `.carabiner/commands/{command}.md`, then execute the contents of that file as the block of operations.".to_owned());
    }
    if simulate_subagents {
        sections.push("## Simulated Subagents\n\nSimulated subagents are specialized AI assistants that can be invoked to handle specific types of tasks. In this case, it can be appear something like custom slash commands simply. Simulated subagents can be called by custom slash commands.\n\nWhen users call a simulated subagent, it will look for the corresponding markdown file, `.carabiner/subagents/{subagent}.md`, and execute its contents as the block of operations.\n\nFor example, if the user instructs `Call planner subagent to plan the refactoring`, you have to look for the markdown file, `.carabiner/subagents/planner.md`, and execute its contents as the block of operations.".to_owned());
    }
    if simulate_skills {
        let entries = model
            .skills
            .iter()
            .filter(|skill| skill.targeted_at(target))
            .map(|skill| {
                let mut entry = Map::new();
                entry.insert("name".into(), Value::String(skill.name.clone()));
                entry.insert(
                    "description".into(),
                    Value::String(
                        get_string(&skill.frontmatter, "description").unwrap_or_default(),
                    ),
                );
                entry.insert(
                    "path".into(),
                    Value::String(format!("@.agents/skills/{}/SKILL.md", skill.name)),
                );
                entry
            })
            .collect::<Vec<_>>();
        if !entries.is_empty() {
            sections.push(format!("## Simulated Skills\n\nSimulated skills are specialized capabilities that can be invoked to handle specific types of tasks. When you determine that a skill would be helpful for the current task, read the corresponding SKILL.md file and execute its instructions.\n\n{}", toon_encode_object_array("skillList", &entries)));
        }
    }
    format!("{}\n\n", sections.join("\n\n"))
}

fn takt_name(frontmatter: &Map<String, Value>, source: &str, feature: &str) -> Result<String> {
    let override_name = frontmatter
        .get("takt")
        .and_then(object_value)
        .and_then(|fields| get_string(fields, "name"));
    let source_stem = source.strip_suffix(".md").unwrap_or(source);
    let name = override_name.unwrap_or_else(|| source_stem.to_owned());
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.split('.').any(|part| part == "..")
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
    {
        return Err(anyhow!("Invalid takt.name \"{name}\" for {feature} \"{source}\": filename stems may not contain path separators or \"..\" segments."));
    }
    Ok(name)
}

fn takt_body(
    frontmatter: &Map<String, Value>,
    body: &str,
    feature: &str,
    source: &str,
) -> Result<String> {
    let Some(extends) = frontmatter
        .get("takt")
        .and_then(object_value)
        .and_then(|fields| get_string(fields, "extends"))
    else {
        return Ok(body.to_owned());
    };
    if extends.is_empty() {
        return Ok(body.to_owned());
    }
    if extends == "."
        || extends == ".."
        || extends.contains('/')
        || extends.contains('\\')
        || extends.split('.').any(|part| part == "..")
        || !extends
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
    {
        return Err(anyhow!("Invalid takt.extends \"{extends}\" for {feature} \"{source}\": the parent must be a bare facet name without path separators or \"..\" segments."));
    }
    let trimmed = body.trim_start();
    if trimmed.is_empty() {
        Ok(format!("{{extends:{extends}}}\n"))
    } else {
        Ok(format!("{{extends:{extends}}}\n\n{trimmed}"))
    }
}

fn render_takt_rule(rule: &Rule) -> Result<(String, String)> {
    let name = takt_name(&rule.frontmatter, &rule.relative_path, "rule")?;
    let facet = rule
        .frontmatter
        .get("takt")
        .and_then(object_value)
        .and_then(|fields| get_string(fields, "facet"));
    let dir = if facet.as_deref() == Some("output-contracts") {
        ".takt/facets/output-contracts"
    } else {
        ".takt/facets/policies"
    };
    Ok((
        format!("{dir}/{name}.md"),
        takt_body(&rule.frontmatter, &rule.body, "rule", &rule.relative_path)?,
    ))
}
fn render_cursor_rule_content(rule: &Rule) -> Result<String> {
    let cursor = rule.frontmatter.get("cursor").and_then(object_value);
    let description = cursor
        .and_then(|fields| get_string(fields, "description"))
        .or_else(|| rule.description().map(ToOwned::to_owned));
    let globs = cursor
        .and_then(|fields| get_string_array(fields, "globs"))
        .unwrap_or_else(|| rule.globs());
    let mut lines = vec!["---".to_owned()];
    if let Some(always_apply) = cursor.and_then(|fields| get_bool(fields, "alwaysApply")) {
        lines.push(format!("alwaysApply: {always_apply}"));
    }
    if let Some(description) = description.filter(|value| !value.is_empty()) {
        let description = description.replace('\n', " ").trim().to_owned();
        let serialized = serde_yaml::to_string(&Map::from_iter([(
            "description".to_owned(),
            Value::String(description),
        )]))?;
        lines.push(serialized.trim_end().to_owned());
    }
    if !globs.is_empty() {
        lines.push(format!("globs: {}", globs.join(",")));
    }
    lines.push("---".to_owned());
    lines.push(String::new());
    if !rule.body.is_empty() {
        lines.push(rule.body.clone());
    }
    Ok(lines.join("\n"))
}

fn render_rule_content(rule: &Rule, target: &str, root: bool) -> Result<String> {
    let body = rule.body.trim();
    if root && target != "cursor" {
        return Ok(body.to_owned());
    }
    if target == "augmentcode" {
        if root {
            return Ok(body.to_owned());
        }
        let mut fm = rule
            .frontmatter
            .get("augmentcode")
            .and_then(object_value)
            .cloned()
            .unwrap_or_default();
        let type_name = get_string(&fm, "type").unwrap_or_else(|| "always_apply".into());
        fm.insert("type".into(), Value::String(type_name));
        let description = rule
            .description()
            .map(ToOwned::to_owned)
            .or_else(|| get_string(&fm, "description"));
        if let Some(description) = description {
            fm.insert("description".into(), Value::String(description));
        }
        return stringify_frontmatter(body, &fm);
    }
    if target == "qwencode" {
        if root {
            return Ok(body.to_owned());
        }
        let globs = rule.globs();
        let specific = globs
            .iter()
            .filter(|glob| !matches!(glob.as_str(), "**/*" | "*" | "**"))
            .cloned()
            .collect::<Vec<_>>();
        let mut fm = Map::new();
        if !specific.is_empty() {
            fm.insert(
                "paths".into(),
                Value::Array(specific.into_iter().map(Value::String).collect()),
            );
        }
        if let Some(description) = rule.description().filter(|value| !value.is_empty()) {
            fm.insert("description".into(), Value::String(description.into()));
        }
        if fm.is_empty() {
            return Ok(body.to_owned());
        }
        return stringify_frontmatter(body, &fm);
    }
    if target == "antigravity-ide" || target == "antigravity-plugin" {
        if root {
            return Ok(body.to_owned());
        }
        let stored = rule
            .frontmatter
            .get("antigravity")
            .and_then(object_value)
            .cloned()
            .unwrap_or_default();
        let stored_trigger = get_string(&stored, "trigger");
        let globs = get_string(&stored, "globs")
            .map(|value| {
                value
                    .split(',')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| rule.globs());
        let specific = globs
            .iter()
            .any(|glob| !matches!(glob.as_str(), "**/*" | "*" | "**"));
        let trigger = stored_trigger.unwrap_or_else(|| {
            if specific {
                "glob".into()
            } else {
                "always_on".into()
            }
        });
        let mut fm = stored;
        fm.insert("trigger".into(), Value::String(trigger.clone()));
        if trigger == "glob" && !globs.is_empty() {
            fm.insert("globs".into(), Value::String(globs.join(",")));
        }
        if trigger == "model_decision" {
            if let Some(description) = rule.description() {
                fm.insert("description".into(), Value::String(description.into()));
            }
        }
        return stringify_frontmatter(body, &fm);
    }
    if matches!(target, "kiro" | "kiro-cli" | "kiro-ide") && !root {
        if rule
            .frontmatter
            .get("agentsmd")
            .and_then(object_value)
            .and_then(|fields| get_string(fields, "subprojectPath"))
            .is_some()
        {
            return Ok(body.to_owned());
        }
        let stored = rule
            .frontmatter
            .get("kiro")
            .and_then(object_value)
            .cloned()
            .unwrap_or_default();
        let globs = rule
            .globs()
            .into_iter()
            .filter(|glob| !matches!(glob.as_str(), "**/*" | "*" | "**"))
            .collect::<Vec<_>>();
        let mut fm = stored.clone();
        if let Some(inclusion) = get_string(&stored, "inclusion") {
            if inclusion == "fileMatch" && !fm.contains_key("fileMatchPattern") && !globs.is_empty()
            {
                fm.insert(
                    "fileMatchPattern".into(),
                    if globs.len() == 1 {
                        Value::String(globs[0].clone())
                    } else {
                        Value::Array(globs.into_iter().map(Value::String).collect())
                    },
                );
            }
        } else if !globs.is_empty() {
            fm.insert("inclusion".into(), Value::String("fileMatch".into()));
            fm.insert(
                "fileMatchPattern".into(),
                if globs.len() == 1 {
                    Value::String(globs[0].clone())
                } else {
                    Value::Array(globs.into_iter().map(Value::String).collect())
                },
            );
        }
        if fm.is_empty() {
            return Ok(body.to_owned());
        }
        return stringify_frontmatter(body, &fm);
    }
    if target == "cursor" {
        return render_cursor_rule_content(rule);
    }
    if target == "claudecode" || target == "claudecode-legacy" {
        let mut fm = Map::new();
        let paths = rule
            .frontmatter
            .get("claudecode")
            .and_then(object_value)
            .and_then(|map| get_string_array(map, "paths"))
            .or_else(|| {
                let globs = rule.globs();
                (!globs.is_empty()).then_some(globs)
            });
        if let Some(paths) = paths {
            fm.insert(
                "paths".into(),
                Value::Array(paths.into_iter().map(Value::String).collect()),
            );
        }
        if fm.is_empty() {
            return Ok(body.to_owned());
        }
        return stringify_frontmatter(body, &fm);
    }
    if target == "amp" {
        let globs = rule.globs();
        if globs.is_empty() {
            return Ok(body.to_owned());
        }
        return stringify_frontmatter(
            body,
            &Map::from_iter([(
                String::from("globs"),
                Value::Array(globs.into_iter().map(Value::String).collect()),
            )]),
        );
    }
    if target == "cline" {
        let globs = rule.globs();
        let mut fm = Map::new();
        if globs.iter().any(|glob| glob == "**/*" || glob == "*") {
            fm.insert("alwaysApply".into(), Value::Bool(true));
        } else if !globs.is_empty() {
            fm.insert(
                "paths".into(),
                Value::Array(globs.into_iter().map(Value::String).collect()),
            );
        }
        if let Some(description) = rule.description() {
            if !description.is_empty() {
                fm.insert("description".into(), Value::String(description.into()));
            }
        }
        if fm.is_empty() {
            return Ok(body.to_owned());
        }
        return stringify_frontmatter(body, &fm);
    }
    if target == "devin" {
        let section = rule.frontmatter.get("devin").and_then(object_value);
        let globs = rule.globs();
        let mut fm = section.cloned().unwrap_or_default();
        if !fm.contains_key("trigger") {
            fm.insert(
                "trigger".into(),
                Value::String(
                    if globs.iter().any(|glob| glob == "**/*" || glob == "*") || globs.is_empty() {
                        "always_on"
                    } else {
                        "glob"
                    }
                    .into(),
                ),
            );
        }
        if !globs.is_empty() && !fm.contains_key("globs") {
            fm.insert("globs".into(), Value::String(globs.join(",")));
        }
        if let Some(description) = rule.description() {
            fm.insert("description".into(), Value::String(description.into()));
        }
        return stringify_frontmatter(body, &fm);
    }
    if target == "copilot" || target == "copilotcli" {
        let mut fm = Map::new();
        if let Some(description) = rule.description() {
            fm.insert("description".into(), Value::String(description.into()));
        }
        let globs = rule.globs();
        if !globs.is_empty() {
            fm.insert("applyTo".into(), Value::String(globs.join(",")));
        }
        if let Some(fields) = rule.frontmatter.get(target).and_then(object_value) {
            for key in ["name", "excludeAgent"] {
                if let Some(value) = fields.get(key) {
                    fm.insert(key.into(), value.clone());
                }
            }
        }
        if fm.is_empty() {
            return Ok(body.to_owned());
        }
        return stringify_frontmatter(body, &fm);
    }
    Ok(body.to_owned())
}

fn render_commands(
    model: &CanonicalModel,
    spec: &TargetSpec,
    global: bool,
    output_root: &Path,
    flattened_naming: &str,
) -> Result<Vec<GeneratedFile>> {
    let paths = spec.paths(global);
    let Some(dir) = &paths.command_dir else {
        return Ok(Vec::new());
    };
    let mut output = Vec::new();
    let commands = model
        .commands
        .iter()
        .filter(|command| command.targeted_at(&spec.name))
        .collect::<Vec<_>>();
    if spec.name == "hermesagent" {
        let mut names = HashSet::new();
        for command in &commands {
            let source = Path::new(&command.relative_path)
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("command");
            let slug = {
                let slug = hermes_slug(source);
                if slug.is_empty() {
                    "command".to_owned()
                } else {
                    slug
                }
            };
            if !names.insert(slug.clone()) {
                return Err(anyhow!("Hermes command slash-name collision: {slug}"));
            }
            if model
                .skills
                .iter()
                .any(|skill| skill.targeted_at("hermesagent") && hermes_slug(&skill.name) == slug)
            {
                return Err(anyhow!(
                    "Hermes command and skill slash-name collision: {slug}"
                ));
            }
        }
    }
    for command in &commands {
        if spec.name == "takt" {
            let name = takt_name(&command.frontmatter, &command.relative_path, "command")?;
            let body = takt_body(
                &command.frontmatter,
                &command.body,
                "command",
                &command.relative_path,
            )?;
            output.push(GeneratedFile::text(
                format!("{dir}/{name}.md"),
                body,
                Feature::Commands,
            ));
            continue;
        }
        if spec.name == "rovodev" {
            let name = command_slug(&command.relative_path);
            let body = command.body.trim().to_owned();
            output.push(GeneratedFile::text(
                format!("{dir}/{name}.md"),
                body,
                Feature::Commands,
            ));
            continue;
        }
        if matches!(spec.name.as_str(), "antigravity-cli" | "antigravity-ide") {
            let (relative, content) = render_antigravity_command(command)?;
            output.push(GeneratedFile::text(
                format!("{dir}/{relative}"),
                content,
                Feature::Commands,
            ));
            continue;
        }
        if spec.name == "goose" {
            output.push(GeneratedFile::text(
                format!(
                    "{dir}/{}",
                    command_file_name(&command.relative_path, "yaml")
                ),
                render_goose_recipe(command)?,
                Feature::Commands,
            ));
            continue;
        }
        if spec.name == "hermesagent" {
            let source = Path::new(&command.relative_path)
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("command");
            let slug = {
                let value = hermes_slug(source);
                if value.is_empty() {
                    "command".into()
                } else {
                    value
                }
            };
            output.push(GeneratedFile::text(
                format!("{dir}/{slug}.json"),
                render_hermes_command(command)?,
                Feature::Commands,
            ));
            continue;
        }
        if spec.name == "warp" {
            let slug = command_slug(&command.relative_path);
            let mut fm = Map::new();
            fm.insert("name".into(), Value::String(slug.clone()));
            fm.insert(
                "description".into(),
                Value::String(
                    get_string(&command.frontmatter, "description")
                        .unwrap_or_else(|| format!("{slug} command")),
                ),
            );
            output.push(GeneratedFile::text(
                format!("{dir}/{slug}/SKILL.md"),
                stringify_frontmatter(&command.body, &fm)?,
                Feature::Commands,
            ));
            continue;
        }
        if spec.name == "devin" {
            let slug = command_slug(&command.relative_path);
            let mut fm = Map::new();
            fm.insert("name".into(), Value::String(slug.clone()));
            fm.insert(
                "description".into(),
                Value::String(
                    get_string(&command.frontmatter, "description")
                        .unwrap_or_else(|| format!("{slug} command")),
                ),
            );
            if let Some(fields) = command.frontmatter.get("devin").and_then(object_value) {
                for key in [
                    "argument-hint",
                    "model",
                    "subagent",
                    "agent",
                    "allowed-tools",
                    "permissions",
                    "triggers",
                ] {
                    if let Some(value) = fields.get(key) {
                        fm.insert(key.into(), value.clone());
                    }
                }
            }
            output.push(GeneratedFile::text(
                format!("{dir}/{slug}/SKILL.md"),
                stringify_frontmatter(&command.body, &fm)?,
                Feature::Commands,
            ));
            continue;
        }
        if matches!(spec.name.as_str(), "kiro" | "kiro-cli" | "kiro-ide") {
            let relative = command_file_name(&command.relative_path, "md");
            output.push(GeneratedFile::text(
                format!("{dir}/{relative}"),
                command.body.trim().to_owned(),
                Feature::Commands,
            ));
            continue;
        }
        let mut relative = if command.relative_path.contains('/')
            && matches!(
                spec.name.as_str(),
                "claudecode"
                    | "claudecode-legacy"
                    | "claudecode-plugin"
                    | "opencode"
                    | "augmentcode"
            ) {
            command.relative_path.clone()
        } else if flattened_naming == "path" && command.relative_path.contains('/') {
            command.relative_path.replace('/', "-")
        } else {
            Path::new(&command.relative_path)
                .file_name()
                .and_then(|v| v.to_str())
                .unwrap_or(&command.relative_path)
                .to_owned()
        };
        if paths.command_ext == "prompt.md" && relative.ends_with(".md") {
            relative.truncate(relative.len() - 3);
            relative.push_str(".prompt.md");
        }
        let mut fm =
            canonical_frontmatter_for_tool(&command.frontmatter, &spec.name, Feature::Commands);
        if let Some(description) = get_string(&command.frontmatter, "description") {
            fm.insert("description".into(), Value::String(description));
        }
        if spec.name == "cline" {
            // Cline workflows are body-only Markdown; it does not consume
            // Carabiner command frontmatter.
            fm.clear();
        }
        let content = if fm.is_empty() {
            command.body.trim().to_owned()
        } else if spec.name == "cursor" {
            stringify_frontmatter_flat(&command.body, &fm)?
        } else {
            stringify_frontmatter(&command.body, &fm)?
        };
        output.push(GeneratedFile::text(
            format!("{dir}/{relative}"),
            content,
            Feature::Commands,
        ));
    }
    let goose_config_path = output_root.join(".config/goose/config.yaml");
    let goose_existing = read_structured_file(&goose_config_path).ok();
    let goose_has_managed = goose_existing
        .as_ref()
        .and_then(|value| value.get("slash_commands"))
        .and_then(Value::as_array)
        .map(|entries| {
            entries.iter().any(|entry| {
                entry
                    .get("recipe_path")
                    .and_then(Value::as_str)
                    .map(|path| path.contains(".config/goose/recipes/"))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false);
    if spec.name == "hermesagent" && !commands.is_empty() {
        output.extend(hermes_plugin_files(global, "commands"));
        if let Some(config) = hermes_enable_plugin(spec, global, output_root, "commands")? {
            output.push(config);
        }
    }
    if spec.name == "goose" && global && (!commands.is_empty() || goose_has_managed) {
        let mut config = goose_existing.unwrap_or(Value::Object(Map::new()));
        let mut entries = commands
            .iter()
            .map(|command| {
                let file = command_file_name(&command.relative_path, "yaml");
                Value::Object(Map::from_iter([
                    (
                        String::from("command"),
                        Value::String(file.trim_end_matches(".yaml").to_ascii_lowercase()),
                    ),
                    (
                        String::from("recipe_path"),
                        Value::String(
                            output_root
                                .join(".config/goose/recipes")
                                .join(file)
                                .to_string_lossy()
                                .replace('\\', "/"),
                        ),
                    ),
                ]))
            })
            .collect::<Vec<_>>();
        if let Some(existing) = config.get("slash_commands").and_then(Value::as_array) {
            entries.splice(
                0..0,
                existing
                    .iter()
                    .filter(|entry| {
                        entry
                            .get("recipe_path")
                            .and_then(Value::as_str)
                            .map(|path| !path.contains(".config/goose/recipes/"))
                            .unwrap_or(true)
                    })
                    .cloned(),
            );
        }
        if let Some(object) = config.as_object_mut() {
            if entries.is_empty() {
                object.remove("slash_commands");
            } else {
                object.insert("slash_commands".into(), Value::Array(entries));
            }
        }
        output.push(GeneratedFile::text(
            ".config/goose/config.yaml",
            serialize_json_or_yaml(&config, "config.yaml")?,
            Feature::Commands,
        ));
    }
    if spec.name == "rovodev" && !commands.is_empty() {
        let manifest_path = output_root.join(".rovodev/prompts.yml");
        let mut manifest = read_structured_or(&manifest_path, Value::Object(Map::new()))?;
        let mut prompts = commands
            .iter()
            .map(|command| {
                let name = command_slug(&command.relative_path);
                Value::Object(Map::from_iter([
                    (String::from("name"), Value::String(name.clone())),
                    (
                        String::from("description"),
                        Value::String(
                            get_string(&command.frontmatter, "description").unwrap_or_default(),
                        ),
                    ),
                    (
                        String::from("content_file"),
                        Value::String(format!("prompts/{name}.md")),
                    ),
                ]))
            })
            .collect::<Vec<_>>();
        prompts.sort_by_key(|value| {
            value
                .as_object()
                .and_then(|object| get_string(object, "name"))
                .unwrap_or_default()
        });
        if let Some(object) = manifest.as_object_mut() {
            object.insert("prompts".into(), Value::Array(prompts));
        } else {
            manifest = Value::Object(Map::from_iter([(
                String::from("prompts"),
                Value::Array(prompts),
            )]));
        }
        output.push(GeneratedFile::text(
            ".rovodev/prompts.yml",
            serialize_json_or_yaml(&manifest, "prompts.yml")?,
            Feature::Commands,
        ));
    }
    Ok(output)
}

fn command_file_name(source: &str, extension: &str) -> String {
    let stem = Path::new(source)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(source);
    format!("{stem}.{extension}")
}

fn command_slug(source: &str) -> String {
    let stem = Path::new(source)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(source);
    stem.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '_' | '-') {
                character
            } else {
                '-'
            }
        })
        .collect()
}
fn render_antigravity_command(command: &Command) -> Result<(String, String)> {
    let source_stem = Path::new(&command.relative_path)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("command");
    let antigravity = command
        .frontmatter
        .get("antigravity")
        .and_then(object_value);
    let trigger = antigravity
        .and_then(|fields| get_string(fields, "trigger"))
        .or_else(|| get_string(&command.frontmatter, "trigger"))
        .or_else(|| {
            command.body.lines().find_map(|line| {
                line.trim()
                    .strip_prefix("trigger: /")
                    .map(|value| format!("/{}", value.trim()))
            })
        })
        .unwrap_or_else(|| format!("/{source_stem}"));
    let sanitized = trigger
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_owned();
    if sanitized.is_empty() {
        return Err(anyhow!(
            "Invalid trigger: sanitization resulted in empty string from \"{trigger}\""
        ));
    }
    let turbo = antigravity
        .and_then(|fields| get_bool(fields, "turbo"))
        .unwrap_or(true);
    let mut fm = Map::new();
    if let Some(description) = get_string(&command.frontmatter, "description") {
        fm.insert("description".into(), Value::String(description));
    }
    fm.insert("trigger".into(), Value::String(trigger.clone()));
    fm.insert("turbo".into(), Value::Bool(turbo));
    let mut body = format!("# Workflow: {}\n\n{}", trigger, command.body.trim());
    if turbo {
        body.push_str("\n\n// turbo");
    }
    Ok((
        format!("{sanitized}.md"),
        stringify_frontmatter(&body, &fm)?,
    ))
}
fn render_goose_recipe(command: &Command) -> Result<String> {
    let title = Path::new(&command.relative_path)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("command");
    let mut recipe = Map::new();
    recipe.insert("version".into(), Value::String("1.0.0".into()));
    recipe.insert("title".into(), Value::String(title.into()));
    recipe.insert(
        "description".into(),
        Value::String(
            get_string(&command.frontmatter, "description").unwrap_or_else(|| title.into()),
        ),
    );
    recipe.insert("prompt".into(), Value::String(command.body.clone()));
    if let Some(Value::Object(fields)) = command.frontmatter.get("goose") {
        for (key, value) in fields {
            recipe.insert(key.clone(), value.clone());
        }
    }
    Ok(format!(
        "{}\n",
        serde_yaml::to_string(&Value::Object(recipe))?.trim_end()
    ))
}
fn hermes_slug(value: &str) -> String {
    let mut result = String::new();
    for character in value.to_ascii_lowercase().chars() {
        if character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-' {
            result.push(character);
        } else if character == '_' || character == ' ' {
            result.push('-');
        }
    }
    let mut compact = String::new();
    for character in result.chars() {
        if character == '-' && compact.ends_with('-') {
            continue;
        }
        compact.push(character);
    }
    compact.trim_matches('-').to_owned()
}

fn render_hermes_command(command: &Command) -> Result<String> {
    let source = Path::new(&command.relative_path)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("command");
    let slug = {
        let value = hermes_slug(source);
        if value.is_empty() {
            "command".into()
        } else {
            value
        }
    };
    let mut spec = Map::new();
    spec.insert("slug".into(), Value::String(slug.clone()));
    spec.insert(
        "description".into(),
        Value::String(
            get_string(&command.frontmatter, "description")
                .unwrap_or_else(|| format!("{slug} command")),
        ),
    );
    spec.insert("prompt".into(), Value::String(command.body.clone()));
    Ok(format!(
        "{}\n",
        serde_json::to_string_pretty(&Value::Object(spec))?
    ))
}

fn hermes_plugin_dir(global: bool, feature: &str) -> String {
    let prefix = if global
        && std::env::var_os("HERMES_HOME")
            .and_then(|value| (!value.is_empty()).then_some(value))
            .is_some()
    {
        "plugins"
    } else {
        ".hermes/plugins"
    };
    format!("{prefix}/carabiner-{feature}")
}

fn hermes_plugin_files(global: bool, feature: &str) -> Vec<GeneratedFile> {
    let directory = hermes_plugin_dir(global, feature);
    let description = match feature {
        "commands" => "Exposes Carabiner commands as Hermes native slash commands.",
        _ => "Exposes Carabiner subagents as Hermes native delegation commands.",
    };
    let init: String = if feature == "commands" {
        r####""""Carabiner-generated Hermes slash commands."""

import json
from pathlib import Path


COMMANDS_DIR = Path(__file__).resolve().parents[2] / "carabiner" / "commands"


def _load_commands():
    if not COMMANDS_DIR.exists():
        return []

    commands = []
    for path in sorted(COMMANDS_DIR.glob("*.json")):
        try:
            command = json.loads(path.read_text(encoding="utf-8"))
        except Exception:
            continue
        if isinstance(command, dict):
            commands.append(command)
    return commands


def _register_command(ctx, command):
    slug = command.get("slug")
    if not slug:
        return
    description = command.get("description") or f"Run the {slug} Carabiner command."
    prompt = command.get("prompt") or ""

    def handler(args=None, **kwargs):
        del kwargs
        raw_args = ""
        if isinstance(args, dict):
            raw_args = args.get("args") or args.get("context") or args.get("prompt") or ""
        elif args is not None:
            raw_args = str(args)
        context_parts = [prompt] if prompt else []
        if raw_args:
            context_parts.append(f"User arguments:\n{raw_args}")
        return ctx.dispatch_tool(
            "delegate_task",
            {"goal": description, "context": "\n\n".join(context_parts)},
        )

    ctx.register_command(slug, handler, description)


def register(ctx):
    for command in _load_commands():
        _register_command(ctx, command)
"####
            .into()
    } else {
        r####""""Carabiner-generated Hermes subagent commands."""

import json
from pathlib import Path


SUBAGENTS_DIR = Path(__file__).resolve().parents[2] / "carabiner" / "subagents"


def _load_subagents():
    if not SUBAGENTS_DIR.exists():
        return []

    subagents = []
    for path in sorted(SUBAGENTS_DIR.glob("*.json")):
        try:
            subagent = json.loads(path.read_text(encoding="utf-8"))
        except Exception:
            continue
        if isinstance(subagent, dict):
            subagent["_path"] = str(path)
            subagents.append(subagent)
    return subagents


def _register_subagent(ctx, subagent):
    slug = subagent.get("slug")
    if not slug:
        return

    command_name = f"carabiner_subagent_{slug}"
    name = subagent.get("name") or slug
    description = subagent.get("description") or f"Delegate work to the {name} Carabiner subagent."
    system_prompt = subagent.get("prompt") or ""

    def handler(args=None, **kwargs):
        del kwargs
        user_context = ""
        if isinstance(args, dict):
            user_context = args.get("context") or args.get("task") or args.get("prompt") or ""
        elif args is not None:
            user_context = str(args)

        context_parts = []
        if system_prompt:
            context_parts.append(system_prompt)
        if user_context:
            context_parts.append(user_context)

        # delegate_task takes no model-facing "toolsets" argument: a subagent
        # inherits the parent's enabled toolsets (role="orchestrator" is the
        # only knob that changes them).
        return ctx.dispatch_tool(
            "delegate_task",
            {
                "goal": description,
                "context": "\n\n".join(context_parts),
            },
        )

    ctx.register_command(command_name, handler, description)


def register(ctx):
    for subagent in _load_subagents():
        _register_subagent(ctx, subagent)
"####
            .into()
    };
    let output_feature = if feature == "commands" {
        Feature::Commands
    } else {
        Feature::Subagents
    };
    vec![
        GeneratedFile::text(
            format!("{directory}/plugin.yaml"),
            format!("name: carabiner-{feature}\nversion: \"1.0.0\"\ndescription: {description}\n"),
            output_feature,
        ),
        GeneratedFile::text(
            format!("{directory}/.carabiner-owned"),
            "Generated and owned by Carabiner.\n".into(),
            output_feature,
        ),
        GeneratedFile::text(format!("{directory}/__init__.py"), init, output_feature),
    ]
}

fn hermes_enable_plugin(
    spec: &TargetSpec,
    global: bool,
    output_root: &Path,
    feature: &str,
) -> Result<Option<GeneratedFile>> {
    let (relative, file) = if global {
        let Some(path) = &spec.paths(global).mcp else {
            return Ok(None);
        };
        (path.path(), path.file.clone())
    } else {
        (".hermes/config.yaml".to_owned(), "config.yaml".to_owned())
    };
    let output_path = output_root.join(&relative);
    let existing = read_structured_or(&output_path, Value::Object(Map::new()))?;
    let mut config = existing.as_object().cloned().unwrap_or_default();
    let plugins = config
        .entry("plugins")
        .or_insert_with(|| Value::Object(Map::new()));
    let plugins = plugins
        .as_object_mut()
        .expect("entry just inserted as Object");
    let enabled = plugins
        .entry("enabled")
        .or_insert_with(|| Value::Array(Vec::new()));
    let enabled = enabled
        .as_array_mut()
        .expect("entry just inserted as Array");
    let plugin = format!("carabiner-{feature}");
    if !enabled
        .iter()
        .any(|value| value.as_str() == Some(plugin.as_str()))
    {
        enabled.push(Value::String(plugin));
    }
    Ok(Some(GeneratedFile::text(
        relative,
        serialize_json_or_yaml(&Value::Object(config), &file)?,
        if feature == "commands" {
            Feature::Commands
        } else {
            Feature::Subagents
        },
    )))
}

fn render_subagents(
    model: &CanonicalModel,
    spec: &TargetSpec,
    global: bool,
    output_root: &Path,
    wildcard_targets: bool,
) -> Result<Vec<GeneratedFile>> {
    let paths = spec.paths(global);
    let Some(dir) = &paths.subagent_dir else {
        return Ok(Vec::new());
    };
    let agents = model
        .subagents
        .iter()
        .filter(|agent| agent.targeted_at(&spec.name))
        .collect::<Vec<_>>();
    if agents.is_empty() {
        return Ok(Vec::new());
    }
    if spec.name == "takt" {
        let mut output = Vec::new();
        for agent in &agents {
            let name = takt_name(&agent.frontmatter, &agent.relative_path, "subagent")?;
            output.push(GeneratedFile::text(
                format!("{dir}/{name}.md"),
                agent.body.trim().to_owned(),
                Feature::Subagents,
            ));
        }
        return Ok(output);
    }
    if spec.name == "hermesagent" {
        let mut output = Vec::new();
        for agent in agents {
            let source = Path::new(&agent.relative_path)
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("agent");
            let slug = {
                let value = hermes_slug(source);
                if value.is_empty() {
                    "agent".into()
                } else {
                    value
                }
            };
            let name = agent.name();
            let description = get_string(&agent.frontmatter, "description")
                .unwrap_or_else(|| format!("Delegate work to the {name} Carabiner subagent."));
            let value = Value::Object(Map::from_iter([
                ("slug".into(), Value::String(slug.clone())),
                ("name".into(), Value::String(name)),
                ("description".into(), Value::String(description)),
                ("prompt".into(), Value::String(agent.body.clone())),
                (
                    "hermes".into(),
                    Value::Object(Map::from_iter([
                        (
                            "command".into(),
                            Value::String(format!("carabiner_subagent_{slug}")),
                        ),
                        ("dispatch".into(), Value::String("delegate_task".into())),
                    ])),
                ),
            ]));
            output.push(GeneratedFile::text(
                format!("{dir}/{slug}.json"),
                format!("{}\n", serde_json::to_string_pretty(&value)?),
                Feature::Subagents,
            ));
        }
        output.extend(hermes_plugin_files(global, "subagents"));
        output.retain(|file| {
            !file
                .relative_path
                .ends_with("carabiner-subagents/.carabiner-owned")
        });
        if global {
            if let Some(config) = hermes_enable_plugin(spec, global, output_root, "subagents")? {
                output.push(config);
            }
        }
        return Ok(output);
    }
    if spec.name == "vibe" {
        let mut output = Vec::new();
        for agent in agents {
            let source = Path::new(&agent.relative_path)
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("agent");
            let slug = source
                .chars()
                .map(|character| {
                    if character.is_ascii_alphanumeric() || matches!(character, '_' | '-') {
                        character
                    } else {
                        '_'
                    }
                })
                .collect::<String>();
            let mut table = agent
                .frontmatter
                .get("vibe")
                .and_then(object_value)
                .cloned()
                .unwrap_or_default();
            table.remove("system_prompt");
            let agent_type = if get_string(&table, "agent_type").as_deref() == Some("agent") {
                "agent"
            } else {
                "subagent"
            };
            table.insert("agent_type".into(), Value::String(agent_type.into()));
            table.insert(
                "display_name".into(),
                Value::String(get_string(&table, "display_name").unwrap_or_else(|| agent.name())),
            );
            if let Some(description) = get_string(&agent.frontmatter, "description") {
                table.insert("description".into(), Value::String(description));
            }
            let prompt_file = if !agent.body.is_empty() {
                table.insert("system_prompt_id".into(), Value::String(slug.clone()));
                Some(GeneratedFile::text(
                    format!(".vibe/prompts/{slug}.md"),
                    agent.body.clone(),
                    Feature::Subagents,
                ))
            } else {
                None
            };
            let body = toml_pretty(&Value::Object(table))?;
            output.push(GeneratedFile::text(
                format!(
                    "{dir}/{}.toml",
                    agent
                        .relative_path
                        .strip_suffix(".md")
                        .unwrap_or(&agent.relative_path)
                ),
                body,
                Feature::Subagents,
            ));
            if let Some(prompt_file) = prompt_file {
                output.push(prompt_file);
            }
        }
        return Ok(output);
    }
    if spec.name == "reasonix" {
        let mut output = Vec::new();
        for agent in agents {
            let name = agent.name();
            safe_name(&name)?;
            let mut frontmatter =
                canonical_frontmatter_for_tool(&agent.frontmatter, &spec.name, Feature::Subagents);
            frontmatter.insert("name".into(), Value::String(name.clone()));
            frontmatter.insert("invocation".into(), Value::String("manual".into()));
            frontmatter.insert("runAs".into(), Value::String("subagent".into()));
            output.push(GeneratedFile::text(
                format!("{dir}/{name}/SKILL.md"),
                stringify_frontmatter(&agent.body, &frontmatter)?,
                Feature::Subagents,
            ));
        }
        return Ok(output);
    }
    if paths.aggregate_subagents {
        let custom_modes = agents
            .iter()
            .map(|agent| {
                let mut mode = Map::new();
                let name = agent.name();
                mode.insert("slug".into(), Value::String(name.clone()));
                mode.insert("name".into(), Value::String(name));
                mode.insert("roleDefinition".into(), Value::String(agent.body.clone()));
                mode.insert(
                    "groups".into(),
                    Value::Array(
                        ["read", "edit", "command", "mcp"]
                            .into_iter()
                            .map(|value| Value::String(value.into()))
                            .collect(),
                    ),
                );
                if let Some(description) = get_string(&agent.frontmatter, "description") {
                    mode.insert("description".into(), Value::String(description));
                }
                if let Some(Value::Object(roo)) = agent.frontmatter.get("roo") {
                    for key in [
                        "slug",
                        "whenToUse",
                        "roleDefinition",
                        "customInstructions",
                        "groups",
                    ] {
                        if let Some(value) = roo.get(key) {
                            mode.insert(key.into(), value.clone());
                        }
                    }
                }
                if spec.name == "zoocode" {
                    if let Some(Value::Object(zoocode)) = agent.frontmatter.get("zoocode") {
                        if let Some(value) = zoocode.get("allowedMcpServers") {
                            mode.insert("allowedMcpServers".into(), value.clone());
                        }
                    }
                }
                mode
            })
            .collect::<Vec<_>>();
        let mut root = Map::new();
        root.insert(
            "customModes".into(),
            Value::Array(custom_modes.into_iter().map(Value::Object).collect()),
        );
        return Ok(vec![GeneratedFile::text(
            ".roomodes",
            format!(
                "{}\n",
                indent_yaml_sequences(&serde_yaml::to_string(&Value::Object(root))?).trim_end()
            ),
            Feature::Subagents,
        )]);
    }
    let mut output = Vec::new();
    for agent in agents {
        let name = agent.name();
        safe_name(&name)?;
        let relative = if paths.subagent_ext == "SKILL.md" {
            format!("{name}/SKILL.md")
        } else if paths.subagent_ext == "AGENTS.md" {
            format!("{name}/AGENTS.md")
        } else if paths.subagent_ext == "AGENT.md" {
            format!("{name}/AGENT.md")
        } else if paths.subagent_ext == "agent.md" {
            format!("{name}.agent.md")
        } else if paths.subagent_ext == "md" {
            format!("{name}.md")
        } else if paths.subagent_ext == "yaml" {
            format!("{name}.yaml")
        } else if paths.subagent_ext == "toml" {
            format!("{name}.toml")
        } else {
            format!("{name}.{}", paths.subagent_ext)
        };
        let mut fm =
            canonical_frontmatter_for_tool(&agent.frontmatter, &spec.name, Feature::Subagents);
        if spec.name == "claudecode-plugin" {
            for key in ["hooks", "mcpServers", "permissionMode"] {
                fm.remove(key);
            }
            if fm.get("isolation").and_then(Value::as_str) != Some("worktree") {
                fm.remove("isolation");
            }
        }
        if spec.name == "kilo" && !fm.contains_key("mode") {
            fm.insert("mode".into(), Value::String("all".into()));
        }
        if spec.name == "opencode" && !fm.contains_key("mode") {
            fm.insert("mode".into(), Value::String("subagent".into()));
        }
        if matches!(spec.name.as_str(), "kilo" | "opencode") {
            fm = ordered_hook_fields(fm, &["description", "mode", "name"]);
        } else if spec.name == "copilotcli" {
            let order = if wildcard_targets {
                &["name", "description"][..]
            } else {
                &["description", "name"][..]
            };
            fm = ordered_hook_fields(fm, order);
        }
        let content = if paths.subagent_ext == "json" {
            render_json_subagent(agent, &fm)?
        } else if paths.subagent_ext == "toml" {
            if spec.name == "codexcli" {
                render_codex_agent(agent, &fm)?
            } else {
                render_toml_agent(agent, &fm)?
            }
        } else if fm.is_empty() {
            agent.body.trim().to_owned()
        } else if matches!(
            spec.name.as_str(),
            "agentsmd"
                | "antigravity-cli"
                | "antigravity-ide"
                | "antigravity-plugin"
                | "augmentcode"
                | "cline"
                | "cursor"
                | "grokcli"
                | "kimi-code"
                | "qwencode"
                | "rovodev"
        ) {
            stringify_frontmatter_flat(&agent.body, &fm)?
        } else {
            stringify_frontmatter(&agent.body, &fm)?
        };
        output.push(GeneratedFile::text(
            format!("{dir}/{relative}"),
            content,
            Feature::Subagents,
        ));
    }
    Ok(output)
}

fn render_json_subagent(agent: &Subagent, fm: &Map<String, Value>) -> Result<String> {
    let mut value = fm.clone();
    value.insert("name".into(), Value::String(agent.name()));
    value.insert(
        "description".into(),
        fm.get("description").cloned().unwrap_or(Value::Null),
    );
    value.insert("prompt".into(), Value::String(agent.body.clone()));
    Ok(format!(
        "{}\n",
        serde_json::to_string_pretty(&Value::Object(value))?
    ))
}
fn render_toml_agent(agent: &Subagent, fm: &Map<String, Value>) -> Result<String> {
    let mut table = Map::new();
    if let Some(name) = fm.get("name") {
        table.insert("name".into(), name.clone());
    }
    if let Some(description) = fm.get("description") {
        table.insert("description".into(), description.clone());
    }
    table.insert("prompt".into(), Value::String(agent.body.clone()));
    toml_pretty(&Value::Object(table))
}
fn render_codex_agent(agent: &Subagent, fm: &Map<String, Value>) -> Result<String> {
    let mut lines = vec![format!("name = {}", toml_string_literal(&agent.name()))];
    if let Some(description) = fm.get("description").and_then(Value::as_str) {
        lines.push(format!(
            "description = {}",
            toml_string_literal(description)
        ));
    }
    if !agent.body.is_empty() {
        let body = agent.body.trim();
        if body.contains('\n') && !body.contains("'''") {
            lines.push(format!("developer_instructions = '''\n{body}\n'''"));
        } else {
            lines.push(format!(
                "developer_instructions = {}",
                toml_string_literal(body)
            ));
        }
    }
    for key in ["model", "model_reasoning_effort", "sandbox_mode"] {
        if let Some(value) = fm.get(key) {
            if let Some(value) = toml_inline_value(value) {
                lines.push(format!("{key} = {value}"));
            }
        }
    }
    Ok(format!("{}\n", lines.join("\n")))
}
fn agent_skill_scalar(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::Null => "null".into(),
        _ => serde_json::to_string(value).unwrap_or_default(),
    }
}

fn normalize_agent_skill_frontmatter(
    frontmatter: &mut Map<String, Value>,
    section: Option<&Map<String, Value>>,
) {
    for key in ["license", "compatibility", "metadata", "allowed-tools"] {
        frontmatter.remove(key);
    }
    let Some(section) = section else {
        return;
    };
    if let Some(value) = section.get("license").filter(|value| value.is_string()) {
        frontmatter.insert("license".into(), value.clone());
    }
    if let Some(value) = section.get("compatibility") {
        let value = match value {
            Value::Object(fields) => Value::String(
                fields
                    .iter()
                    .map(|(key, value)| format!("{key}: {}", agent_skill_scalar(value)))
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
            Value::String(value) => Value::String(value.clone()),
            _ => Value::String(agent_skill_scalar(value)),
        };
        if value
            .as_str()
            .map(|value| !value.is_empty())
            .unwrap_or(false)
        {
            frontmatter.insert("compatibility".into(), value);
        }
    }
    if let Some(Value::Object(metadata)) = section.get("metadata") {
        let metadata = metadata
            .iter()
            .map(|(key, value)| (key.clone(), Value::String(agent_skill_scalar(value))))
            .collect();
        frontmatter.insert("metadata".into(), Value::Object(metadata));
    }
    if let Some(value) = section.get("allowed-tools") {
        let value = match value {
            Value::Array(values) => values
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" "),
            Value::String(value) => value.clone(),
            _ => String::new(),
        };
        if !value.is_empty() {
            frontmatter.insert("allowed-tools".into(), Value::String(value));
        }
    }
}

fn skill_section(source: &Map<String, Value>, target: &str) -> Map<String, Value> {
    source
        .get(target)
        .and_then(object_value)
        .cloned()
        .unwrap_or_default()
}

fn skill_identity(skill: &Skill) -> (String, String) {
    (
        get_string(&skill.frontmatter, "name").unwrap_or_else(|| skill.name.clone()),
        get_string(&skill.frontmatter, "description").unwrap_or_default(),
    )
}

fn skill_resolved_bool(
    source: &Map<String, Value>,
    section: &Map<String, Value>,
    key: &str,
) -> Option<bool> {
    section
        .get(key)
        .and_then(Value::as_bool)
        .or_else(|| source.get(key).and_then(Value::as_bool))
}

fn skill_allowed_tools(value: Option<&Value>) -> Option<Value> {
    match value {
        Some(Value::Array(values)) => Some(Value::String(
            values
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" "),
        )),
        Some(Value::String(value)) => Some(Value::String(value.clone())),
        _ => None,
    }
}

fn skill_section_without_identity(mut section: Map<String, Value>) -> Map<String, Value> {
    section.remove("name");
    section.remove("description");
    section
}

fn skill_frontmatter_for_target(skill: &Skill, target: &str) -> Map<String, Value> {
    let source = &skill.frontmatter;
    let (name, description) = skill_identity(skill);
    let mut frontmatter = Map::new();
    let put_identity = |frontmatter: &mut Map<String, Value>| {
        frontmatter.insert("name".into(), Value::String(name.clone()));
        frontmatter.insert("description".into(), Value::String(description.clone()));
    };
    match target {
        "agentsskills" | "agentsmd" => {
            put_identity(&mut frontmatter);
            normalize_agent_skill_frontmatter(
                &mut frontmatter,
                source.get("agentsskills").and_then(object_value),
            );
        }
        "hermesagent" => {
            put_identity(&mut frontmatter);
            normalize_agent_skill_frontmatter(
                &mut frontmatter,
                source.get("agentsskills").and_then(object_value),
            );
            if let Some(hermes) = source.get("hermesagent").and_then(object_value) {
                for (key, value) in hermes {
                    if key != "name" && key != "description" {
                        frontmatter.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        "claudecode" | "claudecode-legacy" | "amp" | "aiassistant" | "augmentcode" | "cline"
        | "goose" | "junie" | "musecode" | "reasonix" | "warp" | "antigravity-cli"
        | "antigravity-ide" | "antigravity-plugin" => put_identity(&mut frontmatter),
        "codexcli" => {
            put_identity(&mut frontmatter);
            if let Some(codex) = source.get("codexcli").and_then(object_value) {
                if let Some(short) = codex.get("short-description") {
                    frontmatter.insert(
                        "metadata".into(),
                        Value::Object(Map::from_iter([(
                            "short-description".into(),
                            short.clone(),
                        )])),
                    );
                }
            }
        }
        "copilot" | "copilotcli" => {
            let mut section = skill_section(source, target);
            section.remove("user-invocable");
            section.remove("disable-model-invocation");
            frontmatter = skill_section_without_identity(section);
            put_identity(&mut frontmatter);
            if let Some(value) = skill_resolved_bool(
                source,
                source
                    .get(target)
                    .and_then(object_value)
                    .unwrap_or(&Map::new()),
                "user-invocable",
            ) {
                frontmatter.insert("user-invocable".into(), Value::Bool(value));
            }
            if let Some(value) = skill_resolved_bool(
                source,
                source
                    .get(target)
                    .and_then(object_value)
                    .unwrap_or(&Map::new()),
                "disable-model-invocation",
            ) {
                frontmatter.insert("disable-model-invocation".into(), Value::Bool(value));
            }
        }
        "cursor" => {
            put_identity(&mut frontmatter);
            let section = skill_section(source, target);
            for key in ["paths", "metadata"] {
                if let Some(value) = section.get(key) {
                    frontmatter.insert(key.into(), value.clone());
                }
            }
            let empty = Map::new();
            if let Some(value) = skill_resolved_bool(
                source,
                source.get(target).and_then(object_value).unwrap_or(&empty),
                "disable-model-invocation",
            ) {
                frontmatter.insert("disable-model-invocation".into(), Value::Bool(value));
            }
            if let Some(value) = skill_resolved_bool(
                source,
                source.get(target).and_then(object_value).unwrap_or(&empty),
                "user-invocable",
            ) {
                frontmatter.insert("user-invocable".into(), Value::Bool(value));
            }
        }
        "deepagents" => {
            put_identity(&mut frontmatter);
            let section = skill_section(source, target);
            if let Some(value) = skill_allowed_tools(section.get("allowed-tools")) {
                frontmatter.insert("allowed-tools".into(), value);
            }
            for key in ["license", "compatibility", "metadata"] {
                if let Some(value) = section.get(key) {
                    frontmatter.insert(key.into(), value.clone());
                }
            }
        }
        "devin" => {
            put_identity(&mut frontmatter);
            let section = skill_section(source, target);
            for key in [
                "argument-hint",
                "model",
                "subagent",
                "agent",
                "allowed-tools",
                "permissions",
            ] {
                if let Some(value) = section.get(key) {
                    frontmatter.insert(key.into(), value.clone());
                }
            }
            if let Some(triggers) = section.get("triggers") {
                frontmatter.insert("triggers".into(), triggers.clone());
            } else {
                let disable = source
                    .get("disable-model-invocation")
                    .and_then(Value::as_bool);
                let user = source.get("user-invocable").and_then(Value::as_bool);
                let triggers = match (disable, user) {
                    (Some(true), Some(false)) => Some(Value::Array(Vec::new())),
                    (Some(true), _) => Some(Value::Array(vec![Value::String("user".into())])),
                    (_, Some(false)) => Some(Value::Array(vec![Value::String("model".into())])),
                    _ => None,
                };
                if let Some(triggers) = triggers {
                    frontmatter.insert("triggers".into(), triggers);
                }
            }
        }
        "factorydroid" => {
            let mut section = skill_section_without_identity(skill_section(source, target));
            section.remove("user-invocable");
            section.remove("disable-model-invocation");
            frontmatter.insert("name".into(), Value::String(name.clone()));
            frontmatter.insert("description".into(), Value::String(description.clone()));
            frontmatter.extend(section);
            let empty = Map::new();
            if let Some(value) = skill_resolved_bool(
                source,
                source.get(target).and_then(object_value).unwrap_or(&empty),
                "disable-model-invocation",
            ) {
                frontmatter.insert("disable-model-invocation".into(), Value::Bool(value));
            }
            if let Some(value) = skill_resolved_bool(
                source,
                source.get(target).and_then(object_value).unwrap_or(&empty),
                "user-invocable",
            ) {
                frontmatter.insert("user-invocable".into(), Value::Bool(value));
            }
        }
        "grokcli" => {
            put_identity(&mut frontmatter);
            let section = skill_section(source, target);
            let empty = Map::new();
            if let Some(value) = skill_resolved_bool(
                source,
                source.get(target).and_then(object_value).unwrap_or(&empty),
                "user-invocable",
            ) {
                frontmatter.insert("user-invocable".into(), Value::Bool(value));
            }
            if let Some(value) = skill_resolved_bool(
                source,
                source.get(target).and_then(object_value).unwrap_or(&empty),
                "disable-model-invocation",
            ) {
                frontmatter.insert("disable-model-invocation".into(), Value::Bool(value));
            }
            let _ = section;
        }
        "kilo" => {
            put_identity(&mut frontmatter);
            let section = skill_section(source, target);
            for key in ["allowed-tools", "license", "compatibility", "metadata"] {
                if let Some(value) = section.get(key) {
                    frontmatter.insert(key.into(), value.clone());
                }
            }
        }
        "kimi-code" => {
            put_identity(&mut frontmatter);
            if let Some(value) = source.get("disable-model-invocation") {
                frontmatter.insert("disableModelInvocation".into(), value.clone());
            }
            frontmatter.extend(skill_section_without_identity(skill_section(
                source, target,
            )));
        }
        "kiro" | "kiro-cli" | "kiro-ide" | "roo" | "zoocode" => {
            put_identity(&mut frontmatter);
            frontmatter.extend(skill_section_without_identity(skill_section(
                source, target,
            )));
        }
        "opencode" => {
            put_identity(&mut frontmatter);
            let section = skill_section(source, target);
            for key in ["license", "compatibility", "metadata", "allowed-tools"] {
                if let Some(value) = section.get(key).or_else(|| source.get(key)) {
                    frontmatter.insert(key.into(), value.clone());
                }
            }
        }
        "pi" => {
            put_identity(&mut frontmatter);
            let mut section = skill_section(source, target);
            if let Some(value) = skill_allowed_tools(section.get("allowed-tools")) {
                frontmatter.insert("allowed-tools".into(), value);
            }
            section.remove("allowed-tools");
            section = skill_section_without_identity(section);
            frontmatter.extend(section);
            let empty = Map::new();
            if let Some(value) = skill_resolved_bool(
                source,
                source.get(target).and_then(object_value).unwrap_or(&empty),
                "disable-model-invocation",
            ) {
                frontmatter.insert("disable-model-invocation".into(), Value::Bool(value));
            }
        }
        "qwencode" => {
            put_identity(&mut frontmatter);
            let section = skill_section(source, target);
            for key in [
                "priority",
                "paths",
                "allowedTools",
                "model",
                "hooks",
                "when_to_use",
                "argument-hint",
            ] {
                if let Some(value) = section.get(key) {
                    frontmatter.insert(key.into(), value.clone());
                }
            }
            let empty = Map::new();
            if let Some(value) = skill_resolved_bool(
                source,
                source.get(target).and_then(object_value).unwrap_or(&empty),
                "user-invocable",
            ) {
                frontmatter.insert("user-invocable".into(), Value::Bool(value));
            }
            if let Some(value) = skill_resolved_bool(
                source,
                source.get(target).and_then(object_value).unwrap_or(&empty),
                "disable-model-invocation",
            ) {
                frontmatter.insert("disable-model-invocation".into(), Value::Bool(value));
            }
        }
        "replit" | "rovodev" => {
            put_identity(&mut frontmatter);
            let section = skill_section(source, target);
            if let Some(value) = section.get("allowed-tools") {
                let value = if target == "replit" {
                    skill_allowed_tools(Some(value)).unwrap_or_else(|| value.clone())
                } else {
                    value.clone()
                };
                frontmatter.insert("allowed-tools".into(), value);
            }
            for key in ["license", "compatibility", "metadata"] {
                if let Some(value) = section.get(key) {
                    frontmatter.insert(key.into(), value.clone());
                }
            }
            if target == "replit" {
                for (key, value) in skill_section_without_identity(section) {
                    if !frontmatter.contains_key(&key) && key != "allowed-tools" {
                        frontmatter.insert(key, value);
                    }
                }
            }
        }
        "vibe" => {
            put_identity(&mut frontmatter);
            let section = skill_section(source, target);
            for key in ["license", "compatibility", "metadata"] {
                if let Some(value) = section.get(key).or_else(|| source.get(key)) {
                    frontmatter.insert(key.into(), value.clone());
                }
            }
            if let Some(value) = section.get("allowed-tools") {
                frontmatter.insert("allowed-tools".into(), value.clone());
            }
            let empty = Map::new();
            if let Some(value) = skill_resolved_bool(
                source,
                source.get(target).and_then(object_value).unwrap_or(&empty),
                "user-invocable",
            ) {
                frontmatter.insert("user-invocable".into(), Value::Bool(value));
            }
        }
        "zed" => {
            put_identity(&mut frontmatter);
            let mut section = skill_section_without_identity(skill_section(source, target));
            section.remove("disable-model-invocation");
            frontmatter.extend(section);
            let empty = Map::new();
            if let Some(value) = skill_resolved_bool(
                source,
                source.get(target).and_then(object_value).unwrap_or(&empty),
                "disable-model-invocation",
            ) {
                frontmatter.insert("disable-model-invocation".into(), Value::Bool(value));
            }
        }
        _ => {
            frontmatter = canonical_frontmatter_for_tool(source, target, Feature::Skills);
            frontmatter.insert("name".into(), Value::String(name));
            frontmatter.insert("description".into(), Value::String(description));
        }
    }
    if target == "claudecode-plugin" {
        frontmatter = ordered_hook_fields(frontmatter, &["name", "description"]);
    }
    frontmatter
}

fn render_skills(
    model: &CanonicalModel,
    spec: &TargetSpec,
    global: bool,
) -> Result<Vec<GeneratedFile>> {
    let paths = spec.paths(global);
    let Some(dir) = &paths.skill_dir else {
        return Ok(Vec::new());
    };
    let mut output = Vec::new();
    let skills = model
        .skills
        .iter()
        .filter(|skill| skill.targeted_at(&spec.name))
        .collect::<Vec<_>>();
    for skill in skills {
        if spec.name == "reasonix"
            && skill
                .frontmatter
                .get("reasonix")
                .and_then(object_value)
                .and_then(|fields| get_string(fields, "runAs"))
                .as_deref()
                == Some("subagent")
        {
            continue;
        }
        safe_name(&skill.name)?;
        if spec.name == "takt" {
            let name = takt_name(&skill.frontmatter, &skill.name, "skill")?;
            let body = takt_body(&skill.frontmatter, &skill.body, "skill", &skill.name)?;
            let main_file = GeneratedFile::text(format!("{dir}/{name}.md"), body, Feature::Skills);
            output.push(main_file);
            for file in &skill.other_files {
                safe_relative_path(&file.relative_path)?;
                let companion = GeneratedFile::binary(
                    format!("{dir}/{}", file.relative_path),
                    file.content.clone(),
                    Feature::Skills,
                );
                output.push(companion);
            }
            continue;
        }
        if spec.name == "codexcli" {
            let codex = skill
                .frontmatter
                .get("codexcli")
                .and_then(object_value)
                .cloned()
                .unwrap_or_default();
            let mut codex_fm = Map::new();
            codex_fm.insert("name".into(), Value::String(skill.name.clone()));
            if let Some(description) = get_string(&skill.frontmatter, "description") {
                codex_fm.insert("description".into(), Value::String(description));
            }
            if let Some(short) = get_string(&codex, "short-description") {
                codex_fm.insert(
                    "metadata".into(),
                    Value::Object(Map::from_iter([(
                        String::from("short-description"),
                        Value::String(short),
                    )])),
                );
            }
            let main_file = GeneratedFile::text(
                format!("{dir}/{}/SKILL.md", skill.name),
                stringify_frontmatter(&skill.body, &codex_fm)?,
                Feature::Skills,
            );
            output.push(main_file);
            let has_sidecar = ["interface", "policy", "dependencies"]
                .iter()
                .any(|key| codex.get(*key).is_some());
            if has_sidecar {
                let mut sidecar = Map::new();
                if let Some(Value::Object(interface)) = codex.get("interface") {
                    let mut value = interface.clone();
                    if !value.contains_key("short_description") {
                        if let Some(short) = get_string(&codex, "short-description") {
                            value.insert("short_description".into(), Value::String(short));
                        }
                    }
                    sidecar.insert("interface".into(), Value::Object(value));
                }
                for key in ["policy", "dependencies"] {
                    if let Some(value) = codex.get(key) {
                        sidecar.insert(key.into(), value.clone());
                    }
                }
                output.push(GeneratedFile::text(
                    format!("{dir}/{}/agents/openai.yaml", skill.name),
                    serde_yaml::to_string(&Value::Object(sidecar))?,
                    Feature::Skills,
                ));
            }
            for file in &skill.other_files {
                safe_relative_path(&file.relative_path)?;
                if file.relative_path.replace('\\', "/") == "agents/openai.yaml" {
                    continue;
                }
                let companion = GeneratedFile::binary(
                    format!("{dir}/{}/{}", skill.name, file.relative_path),
                    file.content.clone(),
                    Feature::Skills,
                );
                output.push(companion);
            }
            continue;
        }
        let fm = skill_frontmatter_for_target(skill, &spec.name);
        let mut main = if spec.name == "cursor" {
            stringify_frontmatter_flat(&skill.body, &fm)?
        } else {
            stringify_frontmatter(&skill.body, &fm)?
        };
        if fm.is_empty() {
            main = skill.body.trim().to_owned();
        }
        let main_file = GeneratedFile::text(
            format!("{dir}/{}/SKILL.md", skill.name),
            main,
            Feature::Skills,
        );
        output.push(main_file);
        for file in &skill.other_files {
            safe_relative_path(&file.relative_path)?;
            let companion = GeneratedFile::binary(
                format!("{dir}/{}/{}", skill.name, file.relative_path),
                file.content.clone(),
                Feature::Skills,
            );
            output.push(companion);
        }
    }
    Ok(output)
}

fn render_kimi_mcp_defaults(
    source: &Value,
    spec: &TargetSpec,
    global: bool,
    output_root: &Path,
) -> Result<Option<GeneratedFile>> {
    if !global || spec.name != "kimi-code" {
        return Ok(None);
    }
    let Some(fields) = source.get("kimi-code").and_then(Value::as_object) else {
        return Ok(None);
    };
    let mut defaults = Map::new();
    if let Some(value) = fields
        .get("startupTimeoutMs")
        .filter(|value| value.is_number())
    {
        defaults.insert("startup_timeout_ms".into(), value.clone());
    }
    if let Some(value) = fields
        .get("toolTimeoutMs")
        .filter(|value| value.is_number())
    {
        defaults.insert("tool_timeout_ms".into(), value.clone());
    }
    if defaults.is_empty() {
        return Ok(None);
    }
    let Some(path) = &spec.paths(global).hooks else {
        return Ok(None);
    };
    let path = resolve_json_variant(output_root, path);
    let existing = read_structured_or(&output_root.join(path.path()), Value::Object(Map::new()))?;
    let mut document = existing.as_object().cloned().unwrap_or_default();
    let mut mcp = document
        .get("mcp")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    mcp.extend(defaults);
    document.insert("mcp".into(), Value::Object(mcp));
    Ok(Some(GeneratedFile::text(
        path.path(),
        serialize_json_or_yaml(&Value::Object(document), &path.file)?,
        Feature::Mcp,
    )))
}

fn render_mcp(
    model: &CanonicalModel,
    spec: &TargetSpec,
    global: bool,
    output_root: &Path,
) -> Result<Vec<GeneratedFile>> {
    let Some(source) = model.mcp.as_ref() else {
        return Ok(Vec::new());
    };
    let paths = spec.paths(global);
    let Some(raw_path) = &paths.mcp else {
        return Ok(Vec::new());
    };
    let mcp_path = resolve_json_variant(output_root, raw_path);
    let effective = effective_mcp(source, &spec.name);
    let existing_path = output_root.join(mcp_path.path());
    let existing = read_structured_or(&existing_path, empty_for_format(paths.mcp_format))?;
    let merged = build_mcp_output(&existing, &effective, paths.mcp_format, &spec.name)?;
    let mut output = Vec::new();
    if !(effective.is_empty() && !existing_path.exists() && is_shared_config(paths.mcp_format)) {
        let content = serialize_structured(&merged, paths.mcp_format)?;
        output.push(GeneratedFile::text(mcp_path.path(), content, Feature::Mcp));
    }
    if spec.name == "rovodev" && !global {
        let config_path = output_root.join(".rovodev/config.yml");
        let existing_config = read_structured_or(&config_path, Value::Object(Map::new()))?;
        let mut config = existing_config.as_object().cloned().unwrap_or_default();
        let already_configured = config
            .get("mcp")
            .and_then(Value::as_object)
            .and_then(|mcp| mcp.get("mcpConfigPath"))
            .and_then(Value::as_str)
            == Some(".rovodev/mcp.json");
        if !already_configured {
            let mut mcp = config
                .remove("mcp")
                .and_then(|value| value.as_object().cloned())
                .unwrap_or_default();
            mcp.insert(
                "mcpConfigPath".into(),
                Value::String(".rovodev/mcp.json".into()),
            );
            config.insert("mcp".into(), Value::Object(mcp));
            output.push(GeneratedFile::text(
                ".rovodev/config.yml",
                serialize_json_or_yaml(&Value::Object(config), "config.yml")?,
                Feature::Mcp,
            ));
        }
    }
    if let Some(defaults) = render_kimi_mcp_defaults(source, spec, global, output_root)? {
        output.push(defaults);
    }
    Ok(output)
}

fn effective_mcp(source: &Value, target: &str) -> Map<String, Value> {
    let Some(root) = source.as_object() else {
        return Map::new();
    };
    let mut servers = match root.get("mcpServers") {
        Some(Value::Object(map)) => map.clone(),
        _ => Map::new(),
    };
    let accepted_targets: HashSet<&str> = match target {
        "claudecode-legacy" => ["claudecode", "claudecode-legacy"].into_iter().collect(),
        "kiro" | "kiro-cli" | "kiro-ide" => ["kiro", "kiro-cli", "kiro-ide"].into_iter().collect(),
        "antigravity-ide" | "antigravity-cli" => {
            ["antigravity-ide", "antigravity-cli"].into_iter().collect()
        }
        _ => [target].into_iter().collect(),
    };
    servers.retain(|_, value| {
        if value.get("enabled").and_then(Value::as_bool) == Some(false) {
            return false;
        }
        match value.get("targets") {
            Some(Value::Array(targets)) => targets.iter().any(|item| {
                item.as_str()
                    .map(|name| name == "*" || accepted_targets.contains(name))
                    .unwrap_or(false)
            }),
            _ => true,
        }
    });
    let block_keys: Vec<&str> = match target {
        "claudecode-legacy" => vec!["claudecode"],
        "kiro" | "kiro-cli" | "kiro-ide" => vec!["kiro"],
        "antigravity-ide" | "antigravity-cli" => vec!["antigravity-ide", "antigravity-cli"],
        _ => vec![target],
    };
    for key in block_keys {
        let Some(Value::Object(block)) = root.get(key) else {
            continue;
        };
        let Some(Value::Object(overrides)) = block.get("mcpServers") else {
            continue;
        };
        for (name, value) in overrides {
            if value.is_null() || value.get("enabled").and_then(Value::as_bool) == Some(false) {
                servers.remove(name);
            } else {
                servers.insert(name.clone(), value.clone());
            }
        }
    }
    servers.retain(|_, value| {
        if value.get("enabled").and_then(Value::as_bool) == Some(false) {
            return false;
        }
        match value.get("targets") {
            Some(Value::Array(targets)) => targets.iter().any(|item| {
                item.as_str()
                    .map(|name| name == "*" || accepted_targets.contains(name))
                    .unwrap_or(false)
            }),
            _ => true,
        }
    });
    servers.iter_mut().for_each(|(_, value)| {
        if let Some(object) = value.as_object_mut() {
            for key in [
                "targets",
                "description",
                "exposed",
                "enabled",
                "envVars",
                "experimentalEnvironment",
                "experimental_environment",
            ] {
                object.remove(key);
            }
        }
    });
    let supports_enabled_tools = matches!(
        target,
        "codexcli"
            | "copilotcli"
            | "deepagents"
            | "hermesagent"
            | "kimi-code"
            | "kilo"
            | "opencode"
            | "qwencode"
    );
    let supports_disabled_tools = matches!(
        target,
        "antigravity-cli"
            | "antigravity-ide"
            | "codexcli"
            | "deepagents"
            | "devin"
            | "factorydroid"
            | "hermesagent"
            | "kimi-code"
            | "kilo"
            | "kiro"
            | "kiro-cli"
            | "kiro-ide"
            | "qwencode"
            | "roo"
            | "zoocode"
            | "vibe"
    );
    servers.iter_mut().for_each(|(_, value)| {
        if let Some(object) = value.as_object_mut() {
            if !supports_enabled_tools {
                object.remove("enabledTools");
            }
            if !supports_disabled_tools {
                object.remove("disabledTools");
            }
        }
    });
    servers
}

fn canonical_command_parts(object: &Map<String, Value>) -> (Option<String>, Vec<Value>) {
    match object.get("command") {
        Some(Value::Array(values)) => {
            let mut values = values.clone();
            let command = values
                .first()
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            if !values.is_empty() {
                values.remove(0);
            }
            if let Some(Value::Array(args)) = object.get("args") {
                values.extend(args.clone());
            }
            (command, values)
        }
        Some(Value::String(command)) => (
            Some(command.clone()),
            object
                .get("args")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
        ),
        _ => (None, Vec::new()),
    }
}

fn mcp_vibe_servers(servers: &Map<String, Value>) -> Vec<Value> {
    servers
        .iter()
        .filter_map(|(name, value)| {
            let object = value.as_object()?;
            let mut output = Map::new();
            output.insert("name".into(), Value::String(name.clone()));
            let transport = object
                .get("transport")
                .and_then(Value::as_str)
                .or_else(|| object.get("type").and_then(Value::as_str))
                .map(|value| match value {
                    "sse" => "http",
                    "local" => "stdio",
                    other => other,
                })
                .or_else(|| {
                    if object.get("command").is_some() {
                        Some("stdio")
                    } else if object
                        .get("url")
                        .or_else(|| object.get("httpUrl"))
                        .is_some()
                    {
                        Some("http")
                    } else {
                        None
                    }
                });
            if let Some(transport) = transport {
                output.insert("transport".into(), Value::String(transport.into()));
            }
            let (command, args) = canonical_command_parts(object);
            if let Some(command) = command {
                output.insert("command".into(), Value::String(command));
            }
            if !args.is_empty() {
                output.insert("args".into(), Value::Array(args));
            }
            for key in [
                "url",
                "headers",
                "api_key_env",
                "api_key_header",
                "api_key_format",
                "env",
                "cwd",
                "auth",
                "startup_timeout_sec",
                "tool_timeout_sec",
                "prompt",
                "sampling_enabled",
                "disabled",
            ] {
                if let Some(value) = object.get(key) {
                    output.insert(key.into(), value.clone());
                }
            }
            if !output.contains_key("url") {
                if let Some(value) = object.get("httpUrl") {
                    output.insert("url".into(), value.clone());
                }
            }
            if let Some(value) = object.get("disabledTools") {
                output.insert("disabled_tools".into(), value.clone());
            }
            Some(Value::Object(output))
        })
        .collect()
}

fn mcp_goose_plugin_servers(servers: &Map<String, Value>) -> Map<String, Value> {
    servers
        .iter()
        .filter_map(|(name, value)| {
            let object = value.as_object()?;
            if object
                .get("url")
                .or_else(|| object.get("httpUrl"))
                .is_some()
            {
                return None;
            }
            let (command, args) = canonical_command_parts(object);
            let command = command?;
            let mut output = Map::new();
            output.insert("command".into(), Value::String(command));
            if !args.is_empty() {
                output.insert("args".into(), Value::Array(args));
            }
            if let Some(value) = object.get("env") {
                output.insert("env".into(), value.clone());
            }
            if let Some(value) = object.get("cwd") {
                output.insert("cwd".into(), value.clone());
            }
            Some((name.clone(), Value::Object(output)))
        })
        .collect()
}

fn mcp_goose_servers(servers: &Map<String, Value>) -> Map<String, Value> {
    servers
        .iter()
        .filter_map(|(name, value)| {
            let object = value.as_object()?;
            let mut output = Map::new();
            output.insert("name".into(), Value::String(name.clone()));
            let url = object.get("url").or_else(|| object.get("httpUrl")).cloned();
            let remote = url.is_some() && object.get("command").is_none();
            let transport = object
                .get("type")
                .and_then(Value::as_str)
                .or_else(|| object.get("transport").and_then(Value::as_str));
            output.insert(
                "type".into(),
                Value::String(
                    if remote {
                        if transport == Some("sse") {
                            "sse"
                        } else {
                            "streamable_http"
                        }
                    } else {
                        "stdio"
                    }
                    .into(),
                ),
            );
            if remote {
                if let Some(url) = url {
                    output.insert("uri".into(), url);
                }
                if let Some(headers) = object.get("headers") {
                    output.insert("headers".into(), headers.clone());
                }
            } else {
                let (command, args) = canonical_command_parts(object);
                if let Some(command) = command {
                    output.insert("cmd".into(), Value::String(command));
                }
                if !args.is_empty() {
                    output.insert("args".into(), Value::Array(args));
                }
                if let Some(env) = object.get("env") {
                    output.insert("envs".into(), env.clone());
                }
            }
            output.insert(
                "enabled".into(),
                Value::Bool(object.get("disabled").and_then(Value::as_bool) != Some(true)),
            );
            if let Some(timeout) = object
                .get("timeout")
                .or_else(|| object.get("networkTimeout"))
            {
                output.insert("timeout".into(), timeout.clone());
            }
            Some((name.clone(), Value::Object(output)))
        })
        .collect()
}

fn mcp_hermes_servers(servers: &Map<String, Value>) -> Map<String, Value> {
    servers
        .iter()
        .filter_map(|(name, value)| {
            let object = value.as_object()?;
            let mut output = Map::new();
            let (command, args) = canonical_command_parts(object);
            if let Some(command) = command {
                output.insert("command".into(), Value::String(command));
                if !args.is_empty() {
                    output.insert("args".into(), Value::Array(args));
                }
                if let Some(env) = object.get("env") {
                    output.insert("env".into(), env.clone());
                }
            } else if let Some(url) = object.get("url").or_else(|| object.get("httpUrl")) {
                output.insert("url".into(), url.clone());
                if let Some(headers) = object.get("headers") {
                    output.insert("headers".into(), headers.clone());
                }
                if object
                    .get("type")
                    .and_then(Value::as_str)
                    .or_else(|| object.get("transport").and_then(Value::as_str))
                    == Some("sse")
                {
                    output.insert("transport".into(), Value::String("sse".into()));
                }
            }
            if object.get("disabled").and_then(Value::as_bool) == Some(true) {
                output.insert("enabled".into(), Value::Bool(false));
            }
            if let Some(timeout) = object
                .get("timeout")
                .or_else(|| object.get("networkTimeout"))
            {
                output.insert("timeout".into(), timeout.clone());
            }
            if let Some(enabled) = object.get("enabledTools") {
                output.insert(
                    "tools".into(),
                    Value::Object(Map::from_iter([("include".into(), enabled.clone())])),
                );
            }
            if let Some(disabled) = object.get("disabledTools") {
                let tools = output
                    .entry("tools")
                    .or_insert_with(|| Value::Object(Map::new()));
                if let Some(tools) = tools.as_object_mut() {
                    tools.insert("exclude".into(), disabled.clone());
                }
            }
            for key in [
                "auth",
                "client_cert",
                "client_key",
                "connect_timeout",
                "supports_parallel_tool_calls",
                "oauth",
                "idle_timeout_seconds",
                "max_lifetime_seconds",
                "ssl_verify",
                "skip_preflight",
                "sampling",
                "keepalive_interval",
                "elicitation",
                "trust",
                "identity_header",
            ] {
                if let Some(value) = object.get(key) {
                    output.insert(key.into(), value.clone());
                }
            }
            Some((name.clone(), Value::Object(output)))
        })
        .collect()
}

fn mcp_kimi_servers(servers: &Map<String, Value>) -> Map<String, Value> {
    servers
        .iter()
        .filter_map(|(name, value)| {
            let object = value.as_object()?;
            let raw_transport = object
                .get("transport")
                .or_else(|| object.get("type"))
                .and_then(Value::as_str);
            if raw_transport == Some("ws") {
                return None;
            }
            let (command, args) = canonical_command_parts(object);
            let url = object.get("httpUrl").or_else(|| object.get("url"));
            let transport = match raw_transport {
                Some("local") | Some("stdio") => "stdio",
                Some("sse") => "sse",
                Some("http") | Some("streamable-http") => "http",
                _ if command.is_some() => "stdio",
                _ => "http",
            };
            if (transport == "stdio" && command.is_none())
                || (transport != "stdio" && url.is_none())
            {
                return None;
            }
            let mut output = Map::new();
            output.insert("transport".into(), Value::String(transport.into()));
            if let Some(command) = command {
                output.insert("command".into(), Value::String(command));
            }
            if !args.is_empty() {
                output.insert("args".into(), Value::Array(args));
            }
            if let Some(url) = url {
                output.insert("url".into(), url.clone());
            }
            for key in [
                "env",
                "cwd",
                "headers",
                "bearerTokenEnvVar",
                "enabled",
                "startupTimeoutMs",
                "toolTimeoutMs",
                "enabledTools",
                "disabledTools",
            ] {
                if let Some(value) = object.get(key) {
                    output.insert(key.into(), value.clone());
                }
            }
            if object.get("disabled").and_then(Value::as_bool) == Some(true) {
                output.insert("enabled".into(), Value::Bool(false));
            }
            Some((name.clone(), Value::Object(output)))
        })
        .collect()
}

fn mcp_deepagents_servers(servers: &Map<String, Value>) -> Map<String, Value> {
    servers
        .iter()
        .filter_map(|(name, value)| {
            let object = value.as_object()?;
            let raw_transport = object
                .get("transport")
                .or_else(|| object.get("type"))
                .and_then(Value::as_str);
            if matches!(raw_transport, Some("ws")) {
                return None;
            }
            let (enabled_tools, disabled_tools) = (
                object.get("enabledTools").and_then(Value::as_array),
                object.get("disabledTools").and_then(Value::as_array),
            );
            if enabled_tools.is_some() && disabled_tools.is_some() {
                return None;
            }
            if enabled_tools.is_some_and(Vec::is_empty) {
                return None;
            }
            let mut output = object.clone();
            let authored_transport = object.get("transport").is_some();
            output.remove("type");
            output.remove("transport");
            output.remove("enabledTools");
            output.remove("disabledTools");
            let normalized = match raw_transport {
                Some("local") | Some("stdio") => Some("stdio"),
                Some("streamable-http") | Some("streamable_http") | Some("http") => Some("http"),
                Some("sse") => Some("sse"),
                _ => None,
            };
            if let Some(normalized) = normalized {
                output.insert(
                    if authored_transport {
                        "transport"
                    } else {
                        "type"
                    }
                    .into(),
                    Value::String(normalized.into()),
                );
            }
            if let Some(enabled_tools) = enabled_tools {
                output.insert("allowedTools".into(), Value::Array(enabled_tools.clone()));
            } else if let Some(disabled_tools) = disabled_tools {
                if !disabled_tools.is_empty() {
                    output.insert("disabledTools".into(), Value::Array(disabled_tools.clone()));
                }
            }
            Some((
                name.clone(),
                Value::Object(ordered_hook_fields(
                    output,
                    &[
                        "command",
                        "args",
                        "url",
                        "env",
                        "allowedTools",
                        "disabledTools",
                        "type",
                        "transport",
                    ],
                )),
            ))
        })
        .collect()
}

fn mcp_warp_servers(servers: &Map<String, Value>) -> Map<String, Value> {
    servers
        .iter()
        .filter_map(|(name, value)| {
            let mut output = value.as_object()?.clone();
            if let Some(cwd) = output.remove("cwd") {
                output.entry("working_directory").or_insert(cwd);
            }
            Some((name.clone(), Value::Object(output)))
        })
        .collect()
}
fn mcp_grok_servers(servers: &Map<String, Value>) -> Map<String, Value> {
    servers
        .iter()
        .filter_map(|(name, value)| {
            let mut object = value.as_object()?.clone();
            if object.get("disabled").and_then(Value::as_bool) == Some(true) {
                object.remove("disabled");
                object.insert("enabled".into(), Value::Bool(false));
            }
            Some((name.clone(), Value::Object(object)))
        })
        .collect()
}

fn mcp_reasonix_servers(servers: &Map<String, Value>) -> Map<String, Value> {
    servers
        .iter()
        .filter_map(|(name, value)| {
            let object = value.as_object()?;
            let candidate = object
                .get("type")
                .or_else(|| object.get("transport"))
                .and_then(Value::as_str)
                .map(|value| match value {
                    "local" => "stdio",
                    "streamable-http" => "http",
                    other => other,
                })
                .or_else(|| {
                    if object.get("command").is_some() {
                        Some("stdio")
                    } else if object
                        .get("url")
                        .or_else(|| object.get("httpUrl"))
                        .is_some()
                    {
                        Some("http")
                    } else {
                        None
                    }
                });
            if candidate.is_some_and(|value| !matches!(value, "stdio" | "http" | "sse")) {
                return None;
            }
            let mut output = object.clone();
            output.remove("disabled");
            output.remove("transport");
            output.remove("httpUrl");
            if let Some(candidate) = candidate {
                output.insert("type".into(), Value::String(candidate.into()));
            }
            let (command, args) = canonical_command_parts(object);
            if let Some(command) = command {
                output.insert("command".into(), Value::String(command));
                if !args.is_empty() {
                    output.insert("args".into(), Value::Array(args));
                }
            }
            if !output.contains_key("url") {
                if let Some(url) = object.get("httpUrl") {
                    output.insert("url".into(), url.clone());
                }
            }
            Some((name.clone(), Value::Object(output)))
        })
        .collect()
}
fn mcp_antigravity_servers(servers: &Map<String, Value>) -> Map<String, Value> {
    servers
        .iter()
        .filter_map(|(name, value)| {
            let object = value.as_object()?;
            let mut output = Map::new();
            for key in ["type", "command", "args"] {
                if let Some(value) = object.get(key) {
                    output.insert(key.into(), value.clone());
                }
            }
            if let Some(url) = object.get("url") {
                output.insert("serverUrl".into(), url.clone());
            }
            if let Some(env) = object.get("env") {
                output.insert("env".into(), env.clone());
            }
            for (key, value) in object {
                if !matches!(key.as_str(), "type" | "command" | "args" | "url" | "env") {
                    output.insert(key.clone(), value.clone());
                }
            }
            Some((name.clone(), Value::Object(output)))
        })
        .collect()
}

fn mcp_copilotcli_servers(servers: &Map<String, Value>) -> Map<String, Value> {
    servers
        .iter()
        .filter_map(|(name, value)| {
            let object = value.as_object()?;
            let (command, args) = canonical_command_parts(object);
            let remote = object
                .get("url")
                .or_else(|| object.get("httpUrl"))
                .is_some();
            if !remote && command.is_none() {
                return None;
            }
            let mut output = object.clone();
            output.remove("httpUrl");
            if remote {
                let kind = object
                    .get("type")
                    .and_then(Value::as_str)
                    .or_else(|| object.get("transport").and_then(Value::as_str))
                    .filter(|value| matches!(*value, "http" | "sse"))
                    .unwrap_or("http");
                output.insert("type".into(), Value::String(kind.into()));
                if !output.contains_key("url") {
                    if let Some(url) = object.get("httpUrl") {
                        output.insert("url".into(), url.clone());
                    }
                }
            } else {
                output.insert("type".into(), Value::String("stdio".into()));
                output.insert("command".into(), Value::String(command?));
                if !args.is_empty() {
                    output.insert("args".into(), Value::Array(args));
                } else {
                    output.remove("args");
                }
            }
            if let Some(enabled) = output.remove("enabledTools") {
                output.insert("tools".into(), enabled);
            }
            output.remove("disabledTools");
            Some((name.clone(), Value::Object(output)))
        })
        .collect()
}

fn mcp_zed_servers(servers: &Map<String, Value>) -> Map<String, Value> {
    servers
        .iter()
        .filter_map(|(name, value)| {
            let object = value.as_object()?;
            let raw_transport = object
                .get("type")
                .or_else(|| object.get("transport"))
                .and_then(Value::as_str);
            if matches!(raw_transport, Some("sse" | "ws")) {
                return None;
            }
            let mut output = Map::new();
            for (key, value) in object {
                if !matches!(
                    key.as_str(),
                    "type"
                        | "transport"
                        | "command"
                        | "args"
                        | "env"
                        | "cwd"
                        | "url"
                        | "httpUrl"
                        | "disabled"
                        | "enabled"
                        | "enabledTools"
                        | "disabledTools"
                ) {
                    output.insert(key.clone(), value.clone());
                }
            }
            let (command, args) = canonical_command_parts(object);
            if let Some(command) = command {
                output.insert("command".into(), Value::String(command));
                if !args.is_empty() {
                    output.insert("args".into(), Value::Array(args));
                }
                if let Some(env) = object.get("env") {
                    output.insert("env".into(), env.clone());
                }
            } else if let Some(url) = object.get("httpUrl").or_else(|| object.get("url")) {
                output.insert("url".into(), url.clone());
                if let Some(headers) = object.get("headers") {
                    output.insert("headers".into(), headers.clone());
                }
            }
            if let Some(timeout) = object.get("timeout") {
                output.insert("timeout".into(), timeout.clone());
            }
            if object.get("disabled").and_then(Value::as_bool) == Some(true) {
                output.insert("enabled".into(), Value::Bool(false));
            }
            Some((name.clone(), Value::Object(output)))
        })
        .collect()
}

fn mcp_roo_servers(servers: &Map<String, Value>) -> Map<String, Value> {
    servers
        .iter()
        .map(|(name, value)| {
            let mut object = value.as_object().cloned().unwrap_or_default();
            if object.get("type").and_then(Value::as_str) == Some("http") {
                object.insert("type".into(), Value::String("streamable-http".into()));
            }
            if object.get("transport").and_then(Value::as_str) == Some("http") {
                object.insert("transport".into(), Value::String("streamable-http".into()));
            }
            (name.clone(), Value::Object(object))
        })
        .collect()
}

fn mcp_rovodev_servers(servers: &Map<String, Value>) -> Map<String, Value> {
    servers
        .iter()
        .map(|(name, value)| {
            let mut object = value.as_object().cloned().unwrap_or_default();
            let transport = object.remove("transport").or_else(|| object.remove("type"));
            if let Some(transport) =
                transport.and_then(|value| value.as_str().map(ToOwned::to_owned))
            {
                let transport = match transport.as_str() {
                    "local" => "stdio",
                    "streamable-http" => "http",
                    other => other,
                };
                object.insert("transport".into(), Value::String(transport.into()));
            }
            (
                name.clone(),
                Value::Object(ordered_hook_fields(
                    object,
                    &["command", "args", "url", "env", "headers", "transport"],
                )),
            )
        })
        .collect()
}

fn strip_empty_object_fields(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut cleaned = Map::new();
            for (key, value) in object {
                let value = strip_empty_object_fields(value);
                if value.as_object().is_some_and(Map::is_empty) {
                    continue;
                }
                cleaned.insert(key.clone(), value);
            }
            Value::Object(cleaned)
        }
        other => other.clone(),
    }
}

fn build_mcp_output(
    existing: &Value,
    servers: &Map<String, Value>,
    format: DataFormat,
    target: &str,
) -> Result<Value> {
    let mut result = match existing {
        Value::Object(map) => map.clone(),
        _ => Map::new(),
    };
    let servers = if target == "goose" && matches!(format, DataFormat::JsonMcpServers) {
        mcp_goose_plugin_servers(servers)
    } else {
        match target {
            "antigravity-cli" | "antigravity-ide" | "antigravity-plugin" => {
                mcp_antigravity_servers(servers)
            }
            "copilotcli" => mcp_copilotcli_servers(servers),
            "kimi-code" => mcp_kimi_servers(servers),
            "deepagents" => mcp_deepagents_servers(servers),
            "zed" => mcp_zed_servers(servers),
            "warp" => mcp_warp_servers(servers),
            "roo" | "zoocode" => mcp_roo_servers(servers),
            "rovodev" => mcp_rovodev_servers(servers),
            _ => servers.clone(),
        }
    };
    let servers = if matches!(format, DataFormat::TomlCodex | DataFormat::TomlGrok) {
        servers
            .iter()
            .filter_map(|(name, value)| {
                let value = strip_empty_object_fields(value);
                (!value.as_object().is_some_and(Map::is_empty)).then_some((name.clone(), value))
            })
            .collect::<Map<_, _>>()
    } else {
        servers
    };
    match format {
        DataFormat::JsonServers => {
            result.insert("servers".into(), Value::Object(servers.clone()));
        }
        DataFormat::JsonAmp => {
            result.insert("amp.mcpServers".into(), Value::Object(servers.clone()));
        }
        DataFormat::JsonOpenCode => {
            let mut native = Map::new();
            let mut tools = match result.remove("tools") {
                Some(Value::Object(map)) => map,
                _ => Map::new(),
            };
            for (name, config) in servers {
                let object = config.as_object().cloned().unwrap_or_default();
                let mut output = Map::new();
                let disabled = object
                    .get("disabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let (command, args) = canonical_command_parts(&object);
                if let Some(command) = command {
                    let mut command_values = vec![Value::String(command)];
                    command_values.extend(args);
                    output.insert("type".into(), Value::String("local".into()));
                    output.insert("command".into(), Value::Array(command_values));
                    output.insert("enabled".into(), Value::Bool(!disabled));
                    if let Some(env) = object.get("env") {
                        output.insert("environment".into(), env.clone());
                    }
                } else if let Some(url) = object.get("url") {
                    output.insert("type".into(), Value::String("remote".into()));
                    output.insert("url".into(), url.clone());
                    output.insert("enabled".into(), Value::Bool(!disabled));
                    if let Some(headers) = object.get("headers") {
                        output.insert("headers".into(), headers.clone());
                    }
                }
                if let Some(cwd) = object.get("cwd") {
                    output.insert("cwd".into(), cwd.clone());
                }
                for key in ["timeout", "oauth"] {
                    if let Some(value) = object.get(key) {
                        output.insert(key.into(), value.clone());
                    }
                }
                native.insert(name.clone(), Value::Object(output));
                if let Some(Value::Array(enabled)) = object.get("enabledTools") {
                    for tool in enabled.iter().filter_map(Value::as_str) {
                        tools.insert(format!("{name}_{tool}"), Value::Bool(true));
                    }
                }
                if let Some(Value::Array(disabled_tools)) = object.get("disabledTools") {
                    for tool in disabled_tools.iter().filter_map(Value::as_str) {
                        tools.insert(format!("{name}_{tool}"), Value::Bool(false));
                    }
                }
            }
            result.insert("mcp".into(), Value::Object(native));
            if !tools.is_empty() {
                result.insert("tools".into(), Value::Object(tools));
            }
        }
        DataFormat::TomlCodex => {
            let mut codex_servers = Map::new();
            for (name, config) in servers {
                let mut server = config.as_object().cloned().unwrap_or_default();
                let (command, args) = canonical_command_parts(&server);
                if let Some(command) = command {
                    server.insert("command".into(), Value::String(command));
                    if !args.is_empty() {
                        server.insert("args".into(), Value::Array(args));
                    }
                }
                for key in [
                    "type",
                    "transport",
                    "disabled",
                    "alwaysAllow",
                    "trust",
                    "kiroAutoApprove",
                ] {
                    server.remove(key);
                }
                if let Some(value) = server.remove("enabledTools") {
                    server.insert("enabled_tools".into(), value);
                }
                if let Some(value) = server.remove("disabledTools") {
                    server.insert("disabled_tools".into(), value);
                }
                if let Some(value) = server.remove("envVars") {
                    server.insert("env_vars".into(), value);
                }
                if let Some(Value::Object(oauth)) = server.get_mut("oauth") {
                    if let Some(client_id) = oauth.get("clientId").cloned() {
                        oauth.entry("client_id").or_insert(client_id);
                    }
                }
                if let Some(existing_servers) =
                    existing.get("mcp_servers").and_then(Value::as_object)
                {
                    if let Some(Value::Object(tools)) = existing_servers
                        .get(&name)
                        .and_then(|server| server.get("tools"))
                    {
                        if !server.contains_key("tools") {
                            server.insert("tools".into(), Value::Object(tools.clone()));
                        }
                    }
                }
                let server = match strip_empty_object_fields(&Value::Object(server)) {
                    Value::Object(server) => Value::Object(ordered_hook_fields(
                        server,
                        &[
                            "url",
                            "command",
                            "args",
                            "env_vars",
                            "enabled_tools",
                            "disabled_tools",
                            "cwd",
                            "timeout",
                            "oauth",
                            "tools",
                        ],
                    )),
                    other => other,
                };
                codex_servers.insert(name.clone(), server);
            }
            result.insert("mcp_servers".into(), Value::Object(codex_servers));
        }
        DataFormat::TomlGrok => {
            result.insert(
                "mcp_servers".into(),
                Value::Object(mcp_grok_servers(&servers)),
            );
        }
        DataFormat::TomlReasonix => {
            let plugins = mcp_reasonix_servers(&servers)
                .into_iter()
                .map(|(name, value)| {
                    let mut item = value.as_object().cloned().unwrap_or_default();
                    item.insert("name".into(), Value::String(name));
                    Value::Object(item)
                })
                .collect::<Vec<_>>();
            result.insert("plugins".into(), Value::Array(plugins));
        }
        DataFormat::TomlVibe => {
            let generated = mcp_vibe_servers(&servers);
            let names = generated
                .iter()
                .filter_map(|value| value.get("name").and_then(Value::as_str))
                .collect::<HashSet<_>>();
            let mut merged = Vec::new();
            if let Some(existing) = existing.get("mcp_servers").and_then(Value::as_array) {
                merged.extend(
                    existing
                        .iter()
                        .filter(|value| {
                            value
                                .get("name")
                                .and_then(Value::as_str)
                                .map(|name| !names.contains(name))
                                .unwrap_or(true)
                        })
                        .cloned(),
                );
            }
            merged.extend(generated);
            result.insert("mcp_servers".into(), Value::Array(merged));
        }
        DataFormat::YamlGoose => {
            let mut extensions = existing
                .get("extensions")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            extensions.extend(mcp_goose_servers(&servers));
            result.insert("extensions".into(), Value::Object(extensions));
        }
        DataFormat::YamlHermes => {
            let mut mcp_servers = existing
                .get("mcp_servers")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            mcp_servers.extend(mcp_hermes_servers(&servers));
            result.insert("mcp_servers".into(), Value::Object(mcp_servers));
        }
        DataFormat::YamlTakt => {
            let mut allowed = Map::new();
            let transports = servers
                .values()
                .filter_map(|value| {
                    value
                        .get("type")
                        .and_then(Value::as_str)
                        .or_else(|| value.get("transport").and_then(Value::as_str))
                })
                .collect::<HashSet<_>>();
            for kind in ["stdio", "sse", "http"] {
                allowed.insert(kind.into(), Value::Bool(transports.contains(kind)));
            }
            result.insert("workflow_mcp_servers".into(), Value::Object(allowed));
        }
        DataFormat::JsonMcpServers | DataFormat::JsonGeneric => {
            result.insert(
                if target == "zed" {
                    "context_servers"
                } else {
                    "mcpServers"
                }
                .into(),
                Value::Object(servers.clone()),
            );
        }
    }
    let _ = target;
    Ok(Value::Object(result))
}

fn render_kiro_standalone_hooks(value: &Value) -> Result<String> {
    let mut hooks = Vec::new();
    if let Some(map) = value.as_object() {
        for (event, definitions) in map {
            let trigger = hook_event_name(event, "kiro-ide");
            for definition in definitions.as_array().into_iter().flatten() {
                let Some(fields) = definition.as_object() else {
                    continue;
                };
                let mut action = Map::new();
                if let Some(prompt) = fields.get("prompt") {
                    action.insert("type".into(), Value::String("agent".into()));
                    action.insert("prompt".into(), prompt.clone());
                } else if let Some(command) = fields.get("command") {
                    action.insert("type".into(), Value::String("command".into()));
                    action.insert("command".into(), command.clone());
                } else {
                    continue;
                }
                let mut entry = Map::new();
                entry.insert(
                    "name".into(),
                    Value::String(get_string(fields, "name").unwrap_or_else(|| trigger.clone())),
                );
                entry.insert("trigger".into(), Value::String(trigger.clone()));
                if let Some(value) = fields.get("description") {
                    entry.insert("description".into(), value.clone());
                }
                if let Some(value) = fields.get("matcher") {
                    entry.insert("matcher".into(), value.clone());
                }
                if let Some(value) = fields.get("timeout") {
                    entry.insert("timeout".into(), value.clone());
                }
                entry.insert(
                    "enabled".into(),
                    Value::Bool(
                        fields
                            .get("enabled")
                            .and_then(Value::as_bool)
                            .unwrap_or(true),
                    ),
                );
                entry.insert("action".into(), Value::Object(action));
                entry = ordered_hook_fields(
                    entry,
                    &[
                        "name",
                        "description",
                        "trigger",
                        "matcher",
                        "action",
                        "timeout",
                        "enabled",
                    ],
                );
                hooks.push(Value::Object(entry));
            }
        }
    }
    Ok(format!(
        "{}\n",
        serde_json::to_string_pretty(&Value::Object(Map::from_iter([
            (String::from("version"), Value::String("v1".into())),
            (String::from("hooks"), Value::Array(hooks))
        ])))?
    ))
}

fn js_hook_matcher(matcher: &str) -> String {
    let mut sanitized = matcher
        .chars()
        .filter(|character| !character.is_control())
        .collect::<String>();
    if sanitized == "*" {
        sanitized = ".*".into();
    }
    serde_json::to_string(&sanitized).unwrap_or_else(|_| "\"\"".into())
}

fn js_template_literal(command: &str) -> String {
    command
        .replace('\\', "\\\\")
        .replace('`', "\\`")
        .replace("${", "\\${")
}

fn opencode_hook_supported(event: &str, target: &str) -> bool {
    let supported = [
        "sessionStart",
        "preToolUse",
        "postToolUse",
        "stop",
        "afterFileEdit",
        "beforeShellExecution",
        "afterShellExecution",
        "permissionRequest",
        "preCompact",
        "postCompact",
        "afterError",
        "fileChanged",
        "notification",
        "permissionDenied",
        "beforeSubmitPrompt",
    ];
    supported.contains(&event) && (target != "kilo" || event != "notification")
}

fn pi_hook_supported(event: &str) -> bool {
    [
        "sessionStart",
        "sessionEnd",
        "preToolUse",
        "postToolUse",
        "preModelInvocation",
        "postModelInvocation",
        "beforeSubmitPrompt",
        "stop",
        "preCompact",
        "postCompact",
    ]
    .contains(&event)
}

fn push_js_hook_group(groups: &mut JsHookGroups, event: String, handler: JsHookHandler) {
    if let Some((_, handlers)) = groups.iter_mut().find(|(name, _)| *name == event) {
        handlers.push(handler);
    } else {
        groups.push((event, vec![handler]));
    }
}

fn render_opencode_style_hooks(value: &Value, target: &str) -> Result<String> {
    let mut named: JsHookGroups = Vec::new();
    let mut generic: JsHookGroups = Vec::new();
    if let Some(map) = value.as_object() {
        for (canonical_event, definitions) in map {
            if !opencode_hook_supported(canonical_event, target) {
                continue;
            }
            let (event_name, tool_gate) = match canonical_event.as_str() {
                "beforeShellExecution" => ("tool.execute.before".to_owned(), Some("bash")),
                "afterShellExecution" => ("tool.execute.after".to_owned(), Some("bash")),
                _ => (hook_event_name(canonical_event, target), None),
            };
            let matcher_supported = matches!(
                event_name.as_str(),
                "tool.execute.before"
                    | "tool.execute.after"
                    | "experimental.session.compacting"
                    | "chat.message"
            ) && tool_gate.is_none();
            let property_gate = (canonical_event == "permissionDenied")
                .then_some("event.properties.reply === \"reject\"".to_owned());
            let Some(definitions) = definitions.as_array() else {
                continue;
            };
            for definition in definitions {
                let Some(definition) = definition.as_object() else {
                    continue;
                };
                if definition.get("type").and_then(Value::as_str) == Some("prompt") {
                    continue;
                }
                let Some(command) = definition.get("command").and_then(Value::as_str) else {
                    continue;
                };
                if command.is_empty() {
                    continue;
                }
                let raw_matcher = definition
                    .get("matcher")
                    .and_then(Value::as_str)
                    .filter(|matcher| !matcher.is_empty());
                if raw_matcher.is_some() && !matcher_supported {
                    continue;
                }
                let handler = JsHookHandler {
                    command: command.to_owned(),
                    matcher: raw_matcher.map(ToOwned::to_owned),
                    tool_gate: tool_gate.map(str::to_owned),
                    property_gate: property_gate.clone(),
                };
                if matches!(
                    event_name.as_str(),
                    "tool.execute.before"
                        | "tool.execute.after"
                        | "experimental.session.compacting"
                        | "chat.message"
                ) {
                    push_js_hook_group(&mut named, event_name.clone(), handler);
                } else {
                    push_js_hook_group(&mut generic, event_name.clone(), handler);
                }
            }
        }
    }

    let mut body = Vec::new();
    if !generic.is_empty() {
        body.push("    event: async ({ event }) => {".to_owned());
        for (index, (event, handlers)) in generic.iter().enumerate() {
            let prefix = if index == 0 { "if" } else { "else if" };
            body.push(format!(
                "      {prefix} (event.type === {}) {{",
                serde_json::to_string(event)?
            ));
            for handler in handlers {
                let command = js_template_literal(&handler.command);
                if let Some(gate) = &handler.property_gate {
                    body.push(format!("        if ({gate}) {{"));
                    body.push(format!("          await $`{command}`;"));
                    body.push("        }".into());
                } else {
                    body.push(format!("        await $`{command}`;"));
                }
            }
            body.push("      }".into());
        }
        body.push("    },".into());
    }
    for (event, handlers) in &named {
        body.push(format!(
            "    {}: async (input) => {{",
            serde_json::to_string(event)?
        ));
        for handler in handlers {
            let command = js_template_literal(&handler.command);
            if let Some(tool) = &handler.tool_gate {
                body.push(format!("      if (input.tool === {tool:?}) {{"));
                body.push(format!("        await $`{command}`;"));
                body.push("      }".into());
            } else if let Some(matcher) = &handler.matcher {
                body.push("      {".into());
                body.push(format!(
                    "        const __re = new RegExp({});",
                    js_hook_matcher(matcher)
                ));
                body.push("        if (__re.test(input.tool)) {".to_owned());
                body.push(format!("          await $`{command}`;"));
                body.push("        }".into());
                body.push("      }".into());
            } else {
                body.push(format!("      await $`{command}`;"));
            }
        }
        body.push("    },".into());
    }

    let mut lines = if target == "kilo" {
        vec![
            "export default {".to_owned(),
            "  id: \"carabiner-hooks\",".to_owned(),
            "  server: async ({ $ }) => {".to_owned(),
            "    return {".to_owned(),
        ]
    } else {
        vec![
            "export const CarabinerHooksPlugin = async ({ $ }) => {".to_owned(),
            "  return {".to_owned(),
        ]
    };
    let body_indent = if target == "kilo" { "  " } else { "" };
    lines.extend(body.into_iter().map(|line| format!("{body_indent}{line}")));
    if target == "kilo" {
        lines.extend([
            "    };".to_owned(),
            "  },".to_owned(),
            "};".to_owned(),
            String::new(),
        ]);
    } else {
        lines.extend(["  };".to_owned(), "};".to_owned(), String::new()]);
    }
    Ok(lines.join("\n"))
}

fn render_pi_hooks(value: &Value) -> Result<String> {
    let mut groups: JsHookGroups = Vec::new();
    if let Some(map) = value.as_object() {
        for (canonical_event, definitions) in map {
            if !pi_hook_supported(canonical_event) {
                continue;
            }
            let event_name = hook_event_name(canonical_event, "pi");
            let Some(definitions) = definitions.as_array() else {
                continue;
            };
            for definition in definitions {
                let Some(definition) = definition.as_object() else {
                    continue;
                };
                if definition.get("type").and_then(Value::as_str) == Some("prompt") {
                    continue;
                }
                let Some(command) = definition.get("command").and_then(Value::as_str) else {
                    continue;
                };
                if command.is_empty() {
                    continue;
                }
                push_js_hook_group(
                    &mut groups,
                    event_name.clone(),
                    JsHookHandler {
                        command: command.to_owned(),
                        matcher: definition
                            .get("matcher")
                            .and_then(Value::as_str)
                            .filter(|matcher| !matcher.is_empty())
                            .map(ToOwned::to_owned),
                        tool_gate: None,
                        property_gate: None,
                    },
                );
            }
        }
    }
    let has_prompt_gate = groups.iter().any(|(event, _)| event == "input");
    let has_blocking_gate = groups
        .iter()
        .any(|(event, _)| matches!(event.as_str(), "input" | "tool_call"));
    let mut lines = vec!["// Generated by carabiner. Do not edit manually.".to_owned()];
    if groups.is_empty() {
        lines.push("export default function () {}".into());
        lines.push(String::new());
        return Ok(lines.join("\n"));
    }
    lines.push("import { exec } from \"node:child_process\";".into());
    lines.push("import { promisify } from \"node:util\";".into());
    lines.push(String::new());
    if has_prompt_gate {
        lines.push("import type { ExtensionAPI, ExtensionContext } from \"@earendil-works/pi-coding-agent\";".into());
    } else {
        lines.push("import type { ExtensionAPI } from \"@earendil-works/pi-coding-agent\";".into());
    }
    lines.push(String::new());
    lines.push("const run = promisify(exec);".into());
    lines.push(String::new());
    if has_blocking_gate {
        lines.extend([
            "const MAX_BLOCK_REASON_LENGTH = 2000;".into(),
            "const MAX_SCANNED_REASON_LENGTH = 128_000;".into(),
            String::new(),
            "function dropTrailingLoneSurrogate(text: string): string {".into(),
            "  return text.replace(/[\\ud800-\\udbff]$/, \"\");".into(),
            "}".into(),
            String::new(),
            "function toBlockReason(error: unknown): string {".into(),
            "  const result = error as { stdout?: unknown; stderr?: unknown; code?: unknown } | null;".into(),
            "  const stderr = String(result?.stderr ?? \"\").trim();".into(),
            "  const stdout = String(result?.stdout ?? \"\").trim();".into(),
            "  const raw =".into(),
            "    stderr ||".into(),
            "    stdout ||".into(),
            "    (result?.code !== undefined".into(),
            "      ? `Hook command failed with exit code ${result.code}.`".into(),
            "      : error instanceof Error".into(),
            "        ? error.message".into(),
            "        : String(error));".into(),
            "  const scannedTruncated = raw.length > MAX_SCANNED_REASON_LENGTH;".into(),
            "  const scanned = scannedTruncated".into(),
            "    ? dropTrailingLoneSurrogate(raw.slice(0, MAX_SCANNED_REASON_LENGTH))".into(),
            "    : raw;".into(),
            "  const sanitized = scanned".into(),
            "    .replace(/\\r\\n?/g, \"\\n\")".into(),
            "    .replace(/\\u001b\\[[0-9;?]*[\\u0020-\\u002f]*[\\u0040-\\u007e]/g, \"\")".into(),
            "    .replace(/\\u001b\\][^\\u0007\\u001b]{0,256}(?:\\u0007|\\u001b\\\\)/g, \"\")".into(),
            "    .replace(/[\\u0000-\\u0008\\u000b\\u000c\\u000e-\\u001f\\u007f-\\u009f]/g, \"\")".into(),
            "    .replace(/[\\p{Cf}\\p{Zl}\\p{Zp}]/gu, \"\")".into(),
            "    .trim();".into(),
            "  if (!sanitized) return \"Hook command failed.\";".into(),
            "  if (sanitized.length <= MAX_BLOCK_REASON_LENGTH && !scannedTruncated) return sanitized;".into(),
            "  return `${dropTrailingLoneSurrogate(sanitized.slice(0, MAX_BLOCK_REASON_LENGTH))}...`;".into(),
            "}".into(),
        ]);
        if has_prompt_gate {
            lines.extend([
                String::new(),
                "function reportPromptGateFailure(ctx: ExtensionContext, reason: string): void {"
                    .into(),
                "  try {".into(),
                "    if (ctx.hasUI) {".into(),
                "      ctx.ui.notify(reason, \"error\");".into(),
                "      return;".into(),
                "    }".into(),
                "  } catch {".into(),
                "    // The UI channel is best-effort; fall through to stderr.".into(),
                "  }".into(),
                "  console.error(reason);".into(),
                "}".into(),
            ]);
        }
        lines.push(String::new());
    }
    lines.push("export default function (pi: ExtensionAPI) {".into());
    for (event, handlers) in groups {
        let blocking = match event.as_str() {
            "tool_call" => "tool",
            "input" => "prompt",
            _ => "none",
        };
        let uses_tool_name = matches!(event.as_str(), "tool_call" | "tool_result")
            && handlers.iter().any(|handler| handler.matcher.is_some());
        let params = if blocking == "prompt" {
            "event, ctx"
        } else if uses_tool_name || event == "message_end" || blocking != "none" {
            "event"
        } else {
            ""
        };
        lines.push(format!(
            "  pi.on({}, async ({params}) => {{",
            serde_json::to_string(&event)?
        ));
        if event == "message_end" {
            lines.push("    if (event.message.role !== \"assistant\") return;".into());
        }
        if blocking == "prompt" {
            lines.push(
                "    if (event.source === \"extension\") return { action: \"continue\" };".into(),
            );
        }
        for handler in &handlers {
            let gated = uses_tool_name && handler.matcher.is_some();
            if gated {
                lines.push(format!(
                    "    if (new RegExp({}).test(event.toolName)) {{",
                    js_hook_matcher(handler.matcher.as_deref().unwrap_or(""))
                ));
            }
            let indent = if gated { "      " } else { "    " };
            let command = serde_json::to_string(&handler.command)?;
            if blocking == "none" {
                lines.push(format!("{indent}await run({command});"));
            } else {
                lines.push(format!("{indent}try {{"));
                lines.push(format!("{indent}  await run({command});"));
                lines.push(format!("{indent}}} catch (error) {{"));
                if blocking == "tool" {
                    lines.push(format!(
                        "{indent}  return {{ block: true, reason: toBlockReason(error) }};"
                    ));
                } else {
                    lines.push(format!(
                        "{indent}  reportPromptGateFailure(ctx, toBlockReason(error));"
                    ));
                    lines.push(format!("{indent}  return {{ action: \"handled\" }};"));
                }
                lines.push(format!("{indent}}}"));
            }
            if gated {
                lines.push("    }".into());
            }
        }
        if blocking == "prompt" {
            lines.push("    return { action: \"continue\" };".into());
        }
        lines.push("  });".into());
    }
    lines.extend(["}".into(), String::new()]);
    Ok(lines.join("\n"))
}

fn render_js_hooks(value: &Value, target: &str) -> Result<String> {
    if target == "pi" {
        render_pi_hooks(value)
    } else {
        render_opencode_style_hooks(value, target)
    }
}
fn render_kiro_embedded_hooks(value: &Value, existing: &Map<String, Value>) -> Value {
    let mut result = existing.clone();
    let mut generated = Map::new();
    let normalized = normalize_hooks(value, "kiro");
    if let Some(hooks) = normalized.as_object() {
        for (event, definitions) in hooks {
            let Some(definitions) = definitions.as_array() else {
                continue;
            };
            let mut output = Vec::new();
            for definition in definitions {
                let Some(fields) = definition.as_object() else {
                    continue;
                };
                if fields.get("type").and_then(Value::as_str) == Some("prompt")
                    || fields.get("command").is_none()
                {
                    continue;
                }
                let mut entry = fields.clone();
                entry.remove("type");
                if let Some(timeout) = entry.remove("timeout") {
                    if timeout.as_f64().map(|value| value > 0.0).unwrap_or(false) {
                        entry.insert("timeout_ms".into(), timeout);
                    }
                }
                if let Some(cache_ttl) = entry.remove("cacheTtl") {
                    entry.insert("cache_ttl_seconds".into(), cache_ttl);
                }
                entry = ordered_hook_fields(
                    entry,
                    &["command", "matcher", "timeout_ms", "cache_ttl_seconds"],
                );
                output.push(Value::Object(entry));
            }
            if !output.is_empty() {
                generated.insert(event.clone(), Value::Array(output));
            }
        }
    }
    let mut hooks = result
        .remove("hooks")
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    hooks.extend(generated);
    result.insert("hooks".into(), Value::Object(hooks));
    let mut ordered = Map::new();
    for key in ["hooks", "allowedTools", "toolsSettings"] {
        if let Some(value) = result.remove(key) {
            ordered.insert(key.into(), value);
        }
    }
    ordered.extend(result);
    Value::Object(ordered)
}
fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace("'", "'\\''"))
}

fn powershell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace("'", "''"))
}

fn render_cline_hooks(
    hook_value: &Value,
    manifest_path: &str,
    hook_dir: &str,
) -> Result<Vec<GeneratedFile>> {
    let mut commands_by_event: HashMap<String, Vec<String>> = HashMap::new();
    if let Some(map) = hook_value.as_object() {
        for (event, definitions) in map {
            let native_event = match event.as_str() {
                "sessionStart" => "TaskStart",
                "sessionEnd" => "SessionShutdown",
                "preToolUse" => "PreToolUse",
                "postToolUse" => "PostToolUse",
                "beforeSubmitPrompt" => "UserPromptSubmit",
                "preCompact" => "PreCompact",
                "notification" => "Notification",
                "taskCompleted" => "TaskComplete",
                "afterError" => "TaskError",
                _ => continue,
            };
            let Some(definitions) = definitions.as_array() else {
                continue;
            };
            for definition in definitions {
                let Some(object) = definition.as_object() else {
                    continue;
                };
                if object
                    .get("type")
                    .and_then(Value::as_str)
                    .map(|value| value != "command")
                    .unwrap_or(false)
                {
                    continue;
                }
                let Some(command) = object.get("command").and_then(Value::as_str) else {
                    continue;
                };
                let command = command.replace(['\n', '\r', '\0'], "");
                if !command.trim().is_empty() {
                    commands_by_event
                        .entry(native_event.to_owned())
                        .or_default()
                        .push(command);
                }
            }
        }
    }
    let mut events = commands_by_event.keys().cloned().collect::<Vec<_>>();
    events.sort();
    let mut output = vec![GeneratedFile::text(
        manifest_path,
        format!(
            "{}\n",
            serde_json::to_string_pretty(&json!({
                "generatedBy": "carabiner",
                "events": events.clone()
            }))?
        ),
        Feature::Hooks,
    )];
    for event in events {
        let commands = commands_by_event.get(&event).cloned().unwrap_or_default();
        let mut script = vec![
            "#!/bin/bash".to_owned(),
            format!(
                "# {event} hook generated by carabiner — edit .carabiner/hooks.jsonc and regenerate."
            ),
            "# carabiner-owned: cline-hooks".into(),
            String::new(),
            r#"case "${OSTYPE:-$(uname -s 2>/dev/null || true)}" in"#.into(),
            "  msys*|MSYS*|cygwin*|CYGWIN*|MINGW*|mingw*)".into(),
            r#"    printf '{"cancel": false, "contextModification": "", "errorMessage": ""}\n'"#
                .into(),
            "    exit 0".into(),
            "    ;;".into(),
            "esac".into(),
            String::new(),
            "payload=$(cat)".into(),
            "cancel=false".into(),
            "error_message=''".into(),
            String::new(),
        ];
        for command in &commands {
            let quoted = shell_single_quote(command);
            script.push("if [ \"$cancel\" = false ]; then".into());
            script.push(format!("  if bash -n -c {quoted} 2>/dev/null; then"));
            script.push(format!(
                "    hook_stderr=$(printf '%s' \"$payload\" | bash -c {quoted} 2>&1 >/dev/null)"
            ));
            script.push("    hook_status=$?".into());
            script.push("    if [ \"$hook_status\" -eq 2 ]; then".into());
            script.push("      cancel=true".into());
            script.push("      error_message=\"$hook_stderr\"".into());
            script.push("    elif [ \"$hook_status\" -ne 0 ]; then".into());
            script.push(format!(
                "      printf '%s\\n' \"carabiner {event} hook failed (exit $hook_status): $hook_stderr\" >&2"
            ));
            script.push("      error_message=\"$hook_stderr\"".into());
            script.push("    fi".into());
            script.push("  else".into());
            script.push(format!(
                "    error_message=\"carabiner {event} hook command is not valid shell syntax\""
            ));
            script.push("    printf '%s\\n' \"$error_message\" >&2".into());
            script.push("  fi".into());
            script.push("fi".into());
            script.push(String::new());
        }
        script.extend([
            "escape_json() {".into(),
            r#"  printf '%s' "$1" | tr '\n\r\t' '   ' | tr -d '\000-\037' | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g'"#.into(),
            "}".into(),
            String::new(),
            r#"printf '{"cancel": %s, "contextModification": "", "errorMessage": "%s"}\n' "$cancel" "$(escape_json "$error_message")""#.into(),
            String::new(),
        ]);
        output.push(GeneratedFile::text(
            format!("{hook_dir}/{event}"),
            script.join("\n"),
            Feature::Hooks,
        ));
        let mut powershell = vec![
            format!(
                "# {event} hook generated by carabiner — edit .carabiner/hooks.jsonc and regenerate."
            ),
            "# carabiner-owned: cline-hooks".into(),
            String::new(),
            "if ($null -ne $IsWindows -and -not $IsWindows) {".into(),
            r#"  Write-Output '{"cancel": false, "contextModification": "", "errorMessage": ""}'"#
                .into(),
            "  exit 0".into(),
            "}".into(),
            String::new(),
            "$payload = [Console]::In.ReadToEnd()".into(),
            "$cancel = $false".into(),
            "$errorMessage = ''".into(),
            String::new(),
        ];
        for command in commands {
            powershell.push("if (-not $cancel) {".into());
            powershell.push(format!(
                "  $hookStderr = ($payload | & cmd /c {} 2>&1 | Out-String)",
                powershell_single_quote(&command)
            ));
            powershell.push("  $hookStatus = $LASTEXITCODE".into());
            powershell.push("  if ($hookStatus -eq 2) {".into());
            powershell.push("    $cancel = $true".into());
            powershell.push("    $errorMessage = $hookStderr".into());
            powershell.push("  } elseif ($hookStatus -ne 0) {".into());
            powershell.push(format!(
                "    Write-Error {}",
                powershell_single_quote(&format!("carabiner {event} hook failed"))
            ));
            powershell.push("    $errorMessage = $hookStderr".into());
            powershell.push("  }".into());
            powershell.push("}".into());
            powershell.push(String::new());
        }
        powershell.extend([
            "@{".into(),
            "  cancel = $cancel".into(),
            "  contextModification = \"\"".into(),
            "  errorMessage = $errorMessage".into(),
            "} | ConvertTo-Json -Compress".into(),
            String::new(),
        ]);
        output.push(GeneratedFile::text(
            format!("{hook_dir}/{event}.ps1"),
            powershell.join("\n"),
            Feature::Hooks,
        ));
    }
    Ok(output)
}

fn render_amp_hooks(value: &Value) -> Result<String> {
    let mut groups: Vec<AmpHandlerGroup> = Vec::new();
    if let Some(map) = value.as_object() {
        for (event, definitions) in map {
            let amp_event = match event.as_str() {
                "sessionStart" => "session.start",
                "preToolUse" => "tool.call",
                "postToolUse" => "tool.result",
                "beforeSubmitPrompt" => "agent.start",
                "stop" => "agent.end",
                _ => continue,
            };
            let Some(definitions) = definitions.as_array() else {
                continue;
            };
            let mut handlers = Vec::new();
            for definition in definitions {
                let Some(definition) = definition.as_object() else {
                    continue;
                };
                if definition.get("type").and_then(Value::as_str) == Some("prompt") {
                    continue;
                }
                let Some(command) = definition.get("command").and_then(Value::as_str) else {
                    continue;
                };
                let matcher = definition
                    .get("matcher")
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                    .map(|value| {
                        let value = value.replace(['\n', '\r', '\0'], "");
                        if value == "*" {
                            ".*".into()
                        } else {
                            value
                        }
                    });
                if matcher.is_some() && !matches!(amp_event, "tool.call" | "tool.result") {
                    continue;
                }
                handlers.push((command.to_owned(), matcher));
            }
            if !handlers.is_empty() {
                if let Some((_, existing)) = groups.iter_mut().find(|(name, _)| name == amp_event) {
                    existing.extend(handlers);
                } else {
                    groups.push((amp_event.into(), handlers));
                }
            }
        }
    }
    if groups.is_empty() {
        return Ok(
            "// Generated by carabiner. Do not edit manually.\nexport default function () {}\n"
                .into(),
        );
    }
    let mut lines = vec![
        "// Generated by carabiner. Do not edit manually.".into(),
        "import type { PluginAPI } from \"@ampcode/plugin\";".into(),
        "".into(),
        "export default function (amp: PluginAPI) {".into(),
    ];
    for (amp_event, handlers) in groups {
        let uses_tool_name = matches!(amp_event.as_str(), "tool.call" | "tool.result")
            && handlers.iter().any(|(_, matcher)| matcher.is_some());
        let parameter = if uses_tool_name { "event" } else { "_event" };
        lines.push(format!(
            "  amp.on({amp_event:?}, async ({parameter}, ctx) => {{"
        ));
        for (command, matcher) in handlers {
            let indent = if uses_tool_name && matcher.is_some() {
                "      "
            } else {
                "    "
            };
            if uses_tool_name {
                if let Some(matcher) = matcher.as_deref() {
                    lines.push(format!(
                        "    if (new RegExp({}).test(event.tool)) {{",
                        serde_json::to_string(matcher)?
                    ));
                }
            }
            if amp_event == "tool.call" {
                lines.push(format!("{indent}try {{"));
                lines.push(format!(
                    "{indent}  const result = await ctx.$`\u{24}{{{{ raw: {} }}}}`;",
                    serde_json::to_string(&command)?
                ));
                lines.push(format!("{indent}  if (result.exitCode !== 0) {{"));
                lines.push(format!(
                    "{indent}    const message = String(result.stderr).trim() || String(result.stdout).trim() || `Hook command failed with exit code ${{result.exitCode}}.`;"
                ));
                lines.push(format!(
                    "{indent}    return {{ action: \"reject-and-continue\", message }};"
                ));
                lines.push(format!("{indent}  }}"));
                lines.push(format!("{indent}}} catch (error) {{"));
                lines.push(format!(
                    "{indent}  const message = error instanceof Error ? error.message : String(error);"
                ));
                lines.push(format!(
                    "{indent}  return {{ action: \"reject-and-continue\", message }};"
                ));
                lines.push(format!("{indent}}}"));
            } else {
                lines.push(format!(
                    "{indent}await ctx.$`\u{24}{{{{ raw: {} }}}}`;",
                    serde_json::to_string(&command)?
                ));
            }
            if uses_tool_name && matcher.is_some() {
                lines.push("    }".into());
            }
        }
        if amp_event == "tool.call" {
            lines.push("    return { action: \"allow\" };".into());
        } else if amp_event == "agent.start" {
            lines.push("    return {};".into());
        }
        lines.push("  });".into());
    }
    lines.push("}".into());
    lines.push(String::new());
    Ok(lines.join("\n"))
}

fn render_vibe_hooks(value: &Value) -> Result<String> {
    let mut entries = Vec::new();
    let Some(map) = value.as_object() else {
        return Ok("hooks = []\n".into());
    };
    for (event, definitions) in map {
        let vibe_event = match event.as_str() {
            "preToolUse" => "pre_tool",
            "postToolUse" => "post_tool",
            "stop" => "post_agent",
            _ => continue,
        };
        for (index, definition) in definitions.as_array().into_iter().flatten().enumerate() {
            let Some(definition) = definition.as_object() else {
                continue;
            };
            if definition.get("type").and_then(Value::as_str) == Some("prompt") {
                continue;
            }
            let Some(command) = definition.get("command").and_then(Value::as_str) else {
                continue;
            };
            let mut entry = Map::new();
            entry.insert(
                "name".into(),
                Value::String(
                    get_string(definition, "name")
                        .unwrap_or_else(|| format!("{vibe_event}-{index}")),
                ),
            );
            entry.insert("type".into(), Value::String(vibe_event.into()));
            entry.insert("command".into(), Value::String(command.into()));
            if matches!(vibe_event, "pre_tool" | "post_tool") {
                entry.insert(
                    "match".into(),
                    Value::String(
                        get_string(definition, "matcher")
                            .filter(|value| !value.is_empty())
                            .unwrap_or_else(|| "*".into()),
                    ),
                );
                if let Some(value) = definition.get("strict").filter(|value| value.is_boolean()) {
                    entry.insert("strict".into(), value.clone());
                }
            }
            for key in ["timeout", "description"] {
                if let Some(value) = definition.get(key) {
                    entry.insert(key.into(), value.clone());
                }
            }
            entries.push(Value::Object(entry));
        }
    }
    let root = Value::Object(Map::from_iter([("hooks".into(), Value::Array(entries))]));
    toml_pretty(&root)
}

fn render_kimi_hooks(value: &Value, existing: &Value, trusted_directory: &Path) -> Result<String> {
    let mut entries = Vec::new();
    if let Some(map) = value.as_object() {
        for (event, definitions) in map {
            let native_event = match event.as_str() {
                "sessionStart" => "SessionStart",
                "sessionEnd" => "SessionEnd",
                "preToolUse" => "PreToolUse",
                "postToolUse" => "PostToolUse",
                "beforeSubmitPrompt" => "UserPromptSubmit",
                "stop" => "Stop",
                "preCompact" => "PreCompact",
                "postCompact" => "PostCompact",
                _ => continue,
            };
            for definition in definitions.as_array().into_iter().flatten() {
                let Some(definition) = definition.as_object() else {
                    continue;
                };
                if definition.get("type").and_then(Value::as_str) == Some("prompt") {
                    continue;
                }
                let Some(command) = definition.get("command").and_then(Value::as_str) else {
                    continue;
                };
                let directory = trusted_directory.to_string_lossy();
                let command = if cfg!(windows) {
                    format!(
                        "set \"CARABINER_KIMI_HOOK_CWD=1\" && cd /d \"{}\" && {command}",
                        directory.replace('"', "\"\"")
                    )
                } else {
                    format!(
                        "export CARABINER_KIMI_HOOK_CWD=1 && cd -- {} && {command}",
                        shell_single_quote(&directory)
                    )
                };
                let mut entry = Map::new();
                entry.insert("event".into(), Value::String(native_event.into()));
                entry.insert("command".into(), Value::String(command));
                if !matches!(native_event, "Stop" | "SessionHeartbeat" | "Interrupt") {
                    if let Some(matcher) = get_string(definition, "matcher") {
                        if !matcher.is_empty() {
                            entry.insert("matcher".into(), Value::String(matcher));
                        }
                    }
                }
                if let Some(timeout) = definition.get("timeout") {
                    let valid = timeout
                        .as_i64()
                        .is_some_and(|value| (1..=600).contains(&value));
                    if valid {
                        entry.insert("timeout".into(), timeout.clone());
                    }
                }
                entries.push(Value::Object(entry));
            }
        }
    }
    let mut document = existing.as_object().cloned().unwrap_or_default();
    document.insert("hooks".into(), Value::Array(entries));
    Ok(format!(
        "{}\n",
        toml::to_string_pretty(&toml::Value::try_from(Value::Object(document))?)?.trim_end()
    ))
}

fn hermes_native_event(event: &str) -> Option<&str> {
    match event {
        "sessionStart" => Some("on_session_start"),
        "sessionEnd" => Some("on_session_end"),
        "preToolUse" => Some("pre_tool_call"),
        "postToolUse" => Some("post_tool_call"),
        "preModelInvocation" => Some("pre_llm_call"),
        "postModelInvocation" => Some("post_llm_call"),
        "subagentStart" => Some("subagent_start"),
        "subagentStop" => Some("subagent_stop"),
        "pre_tool_call"
        | "post_tool_call"
        | "transform_terminal_output"
        | "transform_tool_result"
        | "transform_llm_output"
        | "pre_llm_call"
        | "post_llm_call"
        | "on_stream_start"
        | "on_stream_delta"
        | "on_stream_end"
        | "on_interim_message"
        | "pre_verify"
        | "pre_api_request"
        | "post_api_request"
        | "api_request_error"
        | "on_session_start"
        | "on_session_end"
        | "on_session_finalize"
        | "on_session_reset"
        | "on_skill_lifecycle"
        | "subagent_start"
        | "subagent_stop"
        | "pre_gateway_dispatch"
        | "pre_approval_request"
        | "post_approval_response"
        | "pre_transcription"
        | "kanban_task_claimed"
        | "kanban_task_completed"
        | "kanban_task_blocked"
        | "on_kanban_worker_spawned"
        | "on_kanban_worker_exited"
        | "on_kanban_worker_stale_claim"
        | "on_kanban_task_updated"
        | "on_kanban_dispatch_tick"
        | "gateway_platform_event"
        | "pre_command" => Some(event),
        _ => None,
    }
}

fn hermes_canonical_event(event: &str) -> Option<&str> {
    match event {
        "on_session_start" => Some("sessionStart"),
        "on_session_end" => Some("sessionEnd"),
        "pre_tool_call" => Some("preToolUse"),
        "post_tool_call" => Some("postToolUse"),
        "pre_llm_call" => Some("preModelInvocation"),
        "post_llm_call" => Some("postModelInvocation"),
        "subagent_start" => Some("subagentStart"),
        "subagent_stop" => Some("subagentStop"),
        _ => None,
    }
}

fn render_hermes_hooks(value: &Value, existing: &Value) -> Result<String> {
    let mut generated = Map::new();
    if let Some(map) = value.as_object() {
        for (event, definitions) in map {
            let Some(native_event) = hermes_native_event(event) else {
                continue;
            };
            let mut entries = Vec::new();
            for definition in definitions.as_array().into_iter().flatten() {
                let Some(definition) = definition.as_object() else {
                    continue;
                };
                if definition.get("type").and_then(Value::as_str) == Some("prompt") {
                    continue;
                }
                let Some(command) = definition.get("command").and_then(Value::as_str) else {
                    continue;
                };
                let mut entry = Map::new();
                entry.insert("command".into(), Value::String(command.into()));
                if matches!(native_event, "pre_tool_call" | "post_tool_call") {
                    if let Some(matcher) = get_string(definition, "matcher") {
                        if !matcher.is_empty() {
                            entry.insert("matcher".into(), Value::String(matcher));
                        }
                    }
                }
                if let Some(timeout) = definition.get("timeout") {
                    entry.insert("timeout".into(), timeout.clone());
                }
                if native_event == "pre_tool_call" {
                    if let Some(value) = definition
                        .get("failClosed")
                        .or_else(|| definition.get("fail_closed"))
                        .filter(|value| value.is_boolean())
                    {
                        entry.insert("fail_closed".into(), value.clone());
                    }
                }
                entries.push(Value::Object(entry));
            }
            if !entries.is_empty() {
                generated.insert(native_event.into(), Value::Array(entries));
            }
        }
    }
    let mut document = existing.as_object().cloned().unwrap_or_default();
    let mut hooks = document
        .get("hooks")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    hooks.extend(generated);
    document.insert("hooks".into(), Value::Object(hooks));
    Ok(format!(
        "{}\n",
        serde_yaml::to_string(&Value::Object(document))?.trim_end()
    ))
}

fn render_reasonix_hooks(value: &Value, existing: &Map<String, Value>) -> Result<String> {
    let mut hooks = Map::new();
    let event_name = |event: &str| match event {
        "sessionStart" => Some("SessionStart"),
        "sessionEnd" => Some("SessionEnd"),
        "preToolUse" => Some("PreToolUse"),
        "postToolUse" => Some("PostToolUse"),
        "beforeSubmitPrompt" => Some("UserPromptSubmit"),
        "stop" => Some("Stop"),
        "subagentStop" => Some("SubagentStop"),
        "postModelInvocation" => Some("PostLLMCall"),
        "notification" => Some("Notification"),
        "preCompact" => Some("PreCompact"),
        _ => None,
    };
    if let Some(map) = value.as_object() {
        for (event, definitions) in map {
            let Some(native_event) = event_name(event) else {
                continue;
            };
            let matcher_event = matches!(native_event, "PreToolUse" | "PostToolUse");
            let mut entries = Vec::new();
            for definition in definitions.as_array().into_iter().flatten() {
                let Some(definition) = definition.as_object() else {
                    continue;
                };
                if definition
                    .get("type")
                    .and_then(Value::as_str)
                    .is_some_and(|kind| kind != "command")
                {
                    continue;
                }
                let Some(command) = definition.get("command").and_then(Value::as_str) else {
                    continue;
                };
                let mut entry = Map::from_iter([("command".into(), Value::String(command.into()))]);
                if matcher_event {
                    if let Some(matcher) =
                        get_string(definition, "matcher").filter(|matcher| !matcher.is_empty())
                    {
                        entry.insert("match".into(), Value::String(matcher));
                    }
                }
                if let Some(description) = get_string(definition, "description")
                    .filter(|description| !description.is_empty())
                {
                    entry.insert("description".into(), Value::String(description));
                }
                if let Some(timeout) = definition.get("timeout").and_then(Value::as_f64) {
                    if timeout.is_finite() {
                        entry.insert(
                            "timeout".into(),
                            Value::Number(
                                serde_json::Number::from_f64((timeout * 1000.0).round())
                                    .unwrap_or_else(|| 0.into()),
                            ),
                        );
                    }
                }
                entries.push(Value::Object(entry));
            }
            if !entries.is_empty() {
                hooks.insert(native_event.into(), Value::Array(entries));
            }
        }
    }
    let mut document = existing.clone();
    document.insert("hooks".into(), Value::Object(hooks));
    Ok(format!(
        "{}\n",
        serde_json::to_string_pretty(&Value::Object(document))?
    ))
}

fn render_copilot_hooks(value: &Value, target: &str) -> Result<String> {
    let mut hooks = Map::new();
    if let Some(map) = value.as_object() {
        for (event, definitions) in map {
            let native_event = hook_event_name(event, target);
            let Some(definitions) = definitions.as_array() else {
                continue;
            };
            let mut entries = Vec::new();
            for definition in definitions {
                let Some(definition) = definition.as_object() else {
                    continue;
                };
                let hook_type = definition
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("command");
                if target == "copilot" && definition.get("matcher").is_some() {
                    continue;
                }
                if target == "copilot" && hook_type != "command" {
                    continue;
                }
                if target == "copilotcli"
                    && !matches!(
                        hook_type,
                        "command" | "prompt" | "http" | "mcp_tool" | "agent"
                    )
                {
                    continue;
                }
                let mut entry = Map::new();
                entry.insert("type".into(), Value::String(hook_type.into()));
                if target == "copilotcli"
                    && definition.get("matcher").is_some()
                    && hook_matcher_supported(target, &native_event)
                {
                    if let Some(matcher) = definition.get("matcher") {
                        entry.insert("matcher".into(), matcher.clone());
                    }
                }
                for key in [
                    "command", "prompt", "url", "headers", "server", "tool", "input", "shell",
                    "cwd",
                ] {
                    if let Some(value) = definition.get(key) {
                        entry.insert(key.into(), value.clone());
                    }
                }
                if let Some(timeout) = definition.get("timeout") {
                    entry.insert("timeoutSec".into(), timeout.clone());
                }
                if entry.get("command").is_none()
                    && entry.get("prompt").is_none()
                    && entry.get("url").is_none()
                    && entry.get("server").is_none()
                {
                    continue;
                }
                entries.push(Value::Object(entry));
            }
            if !entries.is_empty() {
                hooks.insert(native_event, Value::Array(entries));
            }
        }
    }
    let version = value
        .get("version")
        .cloned()
        .unwrap_or_else(|| Value::Number(1.into()));
    Ok(serde_json::to_string_pretty(&Value::Object(Map::from_iter([
        ("version".into(), version),
        ("hooks".into(), Value::Object(hooks)),
    ])))?
        + "\n")
}

fn render_hooks(
    model: &CanonicalModel,
    spec: &TargetSpec,
    global: bool,
    output_root: &Path,
    trusted_directory: &Path,
) -> Result<Vec<GeneratedFile>> {
    let Some(source) = model.hooks.as_ref() else {
        return Ok(Vec::new());
    };
    let paths = spec.paths(global);
    let Some(raw_path) = &paths.hooks else {
        return Ok(Vec::new());
    };
    let path = resolve_json_variant(output_root, raw_path);
    let output_path = output_root.join(path.path());
    let existing = if path.file.ends_with(".js") || path.file.ends_with(".ts") {
        Value::Object(Map::new())
    } else {
        read_structured_or(&output_path, Value::Object(Map::new()))?
    };
    let mut result = match existing {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    let hook_value = effective_hooks(source, &spec.name);
    if spec.name == "hermesagent" {
        return Ok(vec![GeneratedFile::text(
            path.path(),
            render_hermes_hooks(&hook_value, &Value::Object(result.clone()))?,
            Feature::Hooks,
        )]);
    }
    if spec.name == "kimi-code" {
        return Ok(vec![GeneratedFile::text(
            path.path(),
            render_kimi_hooks(
                &hook_value,
                &Value::Object(result.clone()),
                trusted_directory,
            )?,
            Feature::Hooks,
        )]);
    }
    if spec.name == "amp" {
        return Ok(vec![GeneratedFile::text(
            path.path(),
            render_amp_hooks(&hook_value)?,
            Feature::Hooks,
        )]);
    }
    if spec.name == "vibe" {
        return Ok(vec![GeneratedFile::text(
            path.path(),
            render_vibe_hooks(&hook_value)?,
            Feature::Hooks,
        )]);
    }
    if spec.name == "reasonix" {
        return Ok(vec![GeneratedFile::text(
            path.path(),
            render_reasonix_hooks(&hook_value, &result)?,
            Feature::Hooks,
        )]);
    }
    if spec.name == "devin" && !global && path.file == "hooks.v1.json" {
        return Ok(vec![GeneratedFile::text(
            path.path(),
            json_pretty(&Value::Object(nested_hook_events(&hook_value, &spec.name)))?,
            Feature::Hooks,
        )]);
    }
    if spec.name == "factorydroid" {
        return Ok(vec![GeneratedFile::text(
            path.path(),
            json_pretty(&Value::Object(nested_hook_events(&hook_value, &spec.name)))?,
            Feature::Hooks,
        )]);
    }
    if matches!(
        spec.name.as_str(),
        "antigravity-cli" | "antigravity-ide" | "antigravity-plugin"
    ) {
        let hooks = nested_hook_events(&hook_value, &spec.name)
            .into_iter()
            .filter(|(event, _)| {
                matches!(
                    event.as_str(),
                    "PreToolUse" | "PostToolUse" | "PreInvocation" | "PostInvocation" | "Stop"
                )
            })
            .collect::<Map<_, _>>();
        let mut nested = Map::new();
        if !hooks.is_empty() {
            nested.insert("carabiner".into(), Value::Object(hooks));
        }
        return Ok(vec![GeneratedFile::text(
            path.path(),
            json_pretty(&Value::Object(nested))?,
            Feature::Hooks,
        )]);
    }
    if matches!(spec.name.as_str(), "copilot" | "copilotcli") {
        return Ok(vec![GeneratedFile::text(
            path.path(),
            render_copilot_hooks(&hook_value, &spec.name)?,
            Feature::Hooks,
        )]);
    }
    if spec.name == "cline" {
        return render_cline_hooks(&hook_value, &path.path(), &path.dir);
    }
    if spec.name == "kiro" && path.file == "default.json" {
        result = match render_kiro_embedded_hooks(&hook_value, &result) {
            Value::Object(map) => map,
            _ => result,
        };
    } else {
        let hooks = if matches!(
            spec.name.as_str(),
            "claudecode"
                | "claudecode-plugin"
                | "codexcli"
                | "deepagents"
                | "augmentcode"
                | "goose"
                | "grokcli"
                | "devin"
                | "qwencode"
        ) {
            Value::Object(nested_hook_events(&hook_value, &spec.name))
        } else {
            normalize_hooks(&hook_value, &spec.name)
        };
        result.insert("hooks".into(), hooks);
    }
    if matches!(spec.name.as_str(), "kiro-cli" | "kiro-ide") && path.file == "carabiner.json" {
        return Ok(vec![GeneratedFile::text(
            path.path(),
            render_kiro_standalone_hooks(&hook_value)?,
            Feature::Hooks,
        )]);
    }
    if matches!(spec.name.as_str(), "cursor" | "copilot" | "copilotcli") {
        if let Some(version) = source.get("version") {
            result.entry("version").or_insert(version.clone());
        } else if spec.name == "cursor" {
            result.entry("version").or_insert(Value::Number(1.into()));
        }
    }
    if matches!(spec.name.as_str(), "claudecode" | "claudecode-legacy")
        && path.file == "settings.json"
    {
        let mut ordered = Map::new();
        if let Some(value) = result.remove("permissions") {
            ordered.insert("permissions".into(), value);
        }
        if let Some(value) = result.remove("hooks") {
            ordered.insert("hooks".into(), value);
        }
        ordered.extend(result);
        result = ordered;
    } else if spec.name == "cursor" {
        let mut ordered = Map::new();
        if let Some(value) = result.remove("version") {
            ordered.insert("version".into(), value);
        }
        if let Some(value) = result.remove("hooks") {
            ordered.insert("hooks".into(), value);
        }
        ordered.extend(result);
        result = ordered;
    }
    let content = if path.file.ends_with(".js") || path.file.ends_with(".ts") {
        render_js_hooks(&hook_value, &spec.name)?
    } else {
        serialize_json_or_yaml(&Value::Object(result), &path.file)?
    };
    Ok(vec![GeneratedFile::text(
        path.path(),
        content,
        Feature::Hooks,
    )])
}

fn hook_event_name(event: &str, target: &str) -> String {
    let mapped = match target {
        "opencode" | "kilo" => match event {
            "sessionStart" => "session.created",
            "preToolUse" => "tool.execute.before",
            "postToolUse" => "tool.execute.after",
            "stop" => "session.idle",
            "afterFileEdit" => "file.edited",
            "permissionRequest" => "permission.asked",
            "permissionDenied" => "permission.replied",
            "notification" => "tui.toast.show",
            "preCompact" => "experimental.session.compacting",
            "beforeSubmitPrompt" => "chat.message",
            "postCompact" => "session.compacted",
            "afterError" => "session.error",
            "fileChanged" => "file.watcher.updated",
            _ => event,
        },
        "pi" => match event {
            "sessionStart" => "session_start",
            "sessionEnd" => "session_shutdown",
            "preToolUse" => "tool_call",
            "postToolUse" => "tool_result",
            "preModelInvocation" => "context",
            "postModelInvocation" => "message_end",
            "beforeSubmitPrompt" => "input",
            "stop" => "agent_end",
            "preCompact" => "session_before_compact",
            "postCompact" => "session_compact",
            _ => event,
        },
        "amp" => match event {
            "sessionStart" => "session.start",
            "preToolUse" => "tool.call",
            "postToolUse" => "tool.result",
            "beforeSubmitPrompt" => "agent.start",
            "stop" => "agent.end",
            _ => event,
        },
        "vibe" => match event {
            "preToolUse" => "pre_tool",
            "postToolUse" => "post_tool",
            "stop" => "post_agent",
            _ => event,
        },
        "cline" => match event {
            "sessionStart" => "TaskStart",
            "sessionEnd" => "SessionShutdown",
            "preToolUse" => "PreToolUse",
            "postToolUse" => "PostToolUse",
            "beforeSubmitPrompt" => "UserPromptSubmit",
            "preCompact" => "PreCompact",
            "notification" => "Notification",
            "taskCompleted" => "TaskComplete",
            "afterError" => "TaskError",
            _ => event,
        },
        "copilot" => match event {
            "beforeSubmitPrompt" => "userPromptSubmitted",
            "stop" => "agentStop",
            "afterError" => "errorOccurred",
            "userPromptExpansion" => "userPromptTransformed",
            _ => event,
        },
        "copilotcli" => match event {
            "beforeSubmitPrompt" => "userPromptSubmitted",
            "stop" => "agentStop",
            "afterError" => "errorOccurred",
            "userPromptExpansion" => "userPromptTransformed",
            "beforeMCPExecution" => "preMcpToolCall",
            _ => event,
        },
        "cursor" => event,
        "kiro" => match event {
            "sessionStart" => "agentSpawn",
            "sessionEnd" | "stop" => "stop",
            "beforeSubmitPrompt" => "userPromptSubmit",
            "preToolUse" => "preToolUse",
            "postToolUse" => "postToolUse",
            _ => event,
        },
        "kiro-cli" => match event {
            "sessionStart" => "SessionStart",
            "beforeSubmitPrompt" => "UserPromptSubmit",
            "preToolUse" => "PreToolUse",

            "postToolUse" => "PostToolUse",
            "stop" | "sessionEnd" => "Stop",
            _ => event,
        },
        "kiro-ide" => match event {
            "sessionStart" => "SessionStart",
            "beforeSubmitPrompt" => "UserPromptSubmit",
            "preToolUse" => "PreToolUse",
            "postToolUse" => "PostToolUse",
            "stop" => "Stop",
            _ => event,
        },
        "hermesagent" => match event {
            "sessionStart" => "on_session_start",
            "sessionEnd" => "on_session_end",
            "preToolUse" => "pre_tool_call",
            "postToolUse" => "post_tool_call",
            "beforeSubmitPrompt" => "on_user_prompt",
            "stop" => "on_agent_end",
            _ => event,
        },
        _ => match event {
            "beforeSubmitPrompt" => "UserPromptSubmit",
            "preToolUse" => "PreToolUse",
            "postToolUse" => "PostToolUse",
            "postToolUseFailure" => "PostToolUseFailure",
            "preModelInvocation" => "PreInvocation",
            "postModelInvocation" => "PostInvocation",
            _ => event,
        },
    };
    if mapped != event
        || matches!(
            target,
            "copilot" | "copilotcli" | "cursor" | "vibe" | "kiro" | "hermesagent"
        )
    {
        mapped.to_owned()
    } else if matches!(
        target,
        "claudecode"
            | "claudecode-legacy"
            | "codexcli"
            | "qwencode"
            | "grokcli"
            | "reasonix"
            | "factorydroid"
            | "junie"
            | "goose"
            | "devin"
            | "deepagents"
            | "antigravity-cli"
            | "antigravity-ide"
            | "antigravity-plugin"
    ) {
        camel_to_pascal(event)
    } else {
        mapped.to_owned()
    }
}

fn ordered_hook_fields(mut fields: Map<String, Value>, order: &[&str]) -> Map<String, Value> {
    let mut ordered = Map::new();
    for key in order {
        if let Some(value) = fields.remove(*key) {
            ordered.insert((*key).to_owned(), value);
        }
    }
    ordered.extend(fields);
    ordered
}

fn normalize_hook_definition(value: &Value, target: &str) -> Value {
    let Some(raw) = value.as_object() else {
        return value.clone();
    };
    let mut definition = raw.clone();
    if let Some(command) = definition.get("command").and_then(Value::as_str) {
        let trimmed = command.trim_start();
        let relative = trimmed.strip_prefix("./").unwrap_or(trimmed);
        let variable = match target {
            "claudecode" => Some("$CLAUDE_PROJECT_DIR"),
            "claudecode-plugin" => Some("$CLAUDE_PLUGIN_ROOT"),
            "factorydroid" => Some("$FACTORY_PROJECT_DIR"),
            _ => None,
        };
        if let Some(variable) = variable {
            if trimmed.starts_with('.') && !trimmed.starts_with('$') && !trimmed.starts_with('/') {
                definition.insert(
                    "command".into(),
                    Value::String(format!("\"{variable}\"/{relative}")),
                );
            }
        }
    }
    if target == "kiro" {
        if let Some(value) = definition.remove("timeout") {
            definition.insert("timeout_ms".into(), value);
        }
        if let Some(value) = definition.remove("cacheTtl") {
            definition.insert("cache_ttl_seconds".into(), value);
        }
    }
    if target == "codexcli" {
        if let Some(value) = definition.remove("commandWindows") {
            definition.insert("command_windows".into(), value);
        }
        if let Some(value) = definition.remove("additionalContextLimit") {
            definition.insert("additional_context_limit".into(), value);
        }
    }
    if target == "cursor" {
        definition = ordered_hook_fields(
            definition,
            &[
                "type",
                "command",
                "timeout",
                "loop_limit",
                "matcher",
                "prompt",
                "failClosed",
            ],
        );
    }
    Value::Object(definition)
}

fn normalize_hooks(value: &Value, target: &str) -> Value {
    let Some(map) = value.as_object() else {
        return value.clone();
    };
    let mut result = Map::new();
    for (event, definitions) in map {
        let name = hook_event_name(event, target);
        let normalized = if let Value::Array(values) = definitions {
            Value::Array(
                values
                    .iter()
                    .map(|value| normalize_hook_definition(value, target))
                    .collect(),
            )
        } else {
            definitions.clone()
        };
        if let Some(existing) = result.get_mut(&name).and_then(Value::as_array_mut) {
            if let Some(values) = normalized.as_array() {
                existing.extend(values.clone());
            }
        } else {
            result.insert(name, normalized);
        }
    }
    Value::Object(result)
}
fn hook_matcher_supported(target: &str, event: &str) -> bool {
    let canonical = match event {
        "PreToolUse" => "preToolUse",
        "PostToolUse" => "postToolUse",
        "PermissionRequest" => "permissionRequest",
        "PostCompact" => "postCompact",
        "Notification" => "notification",
        "SubagentStart" => "subagentStart",
        "PreCompact" => "preCompact",
        "UserPromptSubmit" => "beforeSubmitPrompt",
        "Stop" => "stop",
        other => other,
    };
    match target {
        "claudecode" | "claudecode-legacy" => !matches!(
            canonical,
            "worktreeCreate"
                | "worktreeRemove"
                | "messageDisplay"
                | "postToolBatch"
                | "taskCreated"
                | "taskCompleted"
                | "teammateIdle"
                | "cwdChanged"
                | "beforeSubmitPrompt"
                | "stop"
        ),
        "augmentcode" | "factorydroid" => matches!(canonical, "preToolUse" | "postToolUse"),
        "antigravity-cli" | "antigravity-ide" | "antigravity-plugin" => {
            matches!(canonical, "preToolUse" | "postToolUse")
        }
        "codexcli" => matches!(
            canonical,
            "preToolUse"
                | "postToolUse"
                | "permissionRequest"
                | "postCompact"
                | "notification"
                | "subagentStart"
        ),
        "copilotcli" => matches!(
            canonical,
            "preToolUse"
                | "postToolUse"
                | "notification"
                | "permissionRequest"
                | "preCompact"
                | "subagentStart"
        ),
        _ => true,
    }
}

fn nested_hook_events(value: &Value, target: &str) -> Map<String, Value> {
    let normalized = normalize_hooks(value, target);
    let Some(events) = normalized.as_object() else {
        return Map::new();
    };
    events
        .iter()
        .filter_map(|(event, definitions)| {
            let values = definitions.as_array()?;
            let mut groups: Vec<(Option<String>, Vec<Value>)> = Vec::new();
            for definition in values {
                let mut inner = definition.as_object().cloned().unwrap_or_default();
                let matcher = inner.remove("matcher").and_then(|value| {
                    value
                        .as_str()
                        .filter(|matcher| {
                            !matcher.is_empty() && hook_matcher_supported(target, event)
                        })
                        .map(ToOwned::to_owned)
                });
                if !inner.contains_key("type") {
                    inner.insert("type".into(), Value::String("command".into()));
                }
                inner = ordered_hook_fields(
                    inner,
                    &[
                        "type",
                        "command",
                        "timeout",
                        "timeout_ms",
                        "cacheTtl",
                        "cache_ttl_seconds",
                        "matcher",
                    ],
                );
                if let Some((_, group)) = groups.iter_mut().find(|(key, _)| *key == matcher) {
                    group.push(Value::Object(inner));
                } else {
                    groups.push((matcher, vec![Value::Object(inner)]));
                }
            }
            let wrapped = groups
                .into_iter()
                .map(|(matcher, hooks)| {
                    let mut entry = Map::new();
                    if let Some(matcher) = matcher {
                        entry.insert("matcher".into(), Value::String(matcher));
                    }
                    entry.insert("hooks".into(), Value::Array(hooks));
                    Value::Object(entry)
                })
                .collect::<Vec<_>>();
            Some((event.clone(), Value::Array(wrapped)))
        })
        .collect()
}

fn camel_to_pascal(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn permission_tool_name(category: &str, target: &str) -> Option<String> {
    if category == "*" {
        return Some("*".into());
    }
    if category.starts_with("mcp__") {
        return Some(if target == "grokcli" {
            format!("MCPTool({})", category.trim_start_matches("mcp__"))
        } else {
            category.into()
        });
    }
    let name = match target {
        "antigravity-cli" | "antigravity-ide" => match category {
            "read" => "read_file",
            "edit" | "write" => "write_file",
            "bash" => "command",
            "webfetch" | "websearch" => "read_url",
            "mcp" => "mcp",
            _ => category,
        },
        "qwen" | "qwencode" => match category {
            "bash" => "Bash",
            "read" => "Read",
            "edit" => "Edit",
            "write" => "Write",
            "webfetch" => "WebFetch",
            "websearch" => "WebSearch",
            "grep" => "Grep",
            "glob" => "Glob",
            "agent" => "Agent",
            _ => category,
        },
        "reasonix" => match category {
            "bash" => "Bash",
            "read" => "Read",
            "edit" => "Edit",
            "write" => "Write",
            "webfetch" => "WebFetch",
            "websearch" => "WebSearch",
            "grep" => "Grep",
            "glob" => "Glob",
            "notebookedit" => "NotebookEdit",
            "agent" => "Agent",
            _ => category,
        },
        "grokcli" => match category {
            "bash" => "Bash",
            "read" => "Read",
            "edit" | "write" => "Edit",
            "grep" => "Grep",
            "webfetch" => "WebFetch",
            "websearch" => "WebSearch",
            "mcp" => "MCPTool",
            _ => return None,
        },
        _ => permission_category(category),
    };
    Some(name.into())
}

fn permission_entry(tool: &str, pattern: &str) -> String {
    if pattern == "*" || pattern.is_empty() {
        tool.into()
    } else {
        format!("{tool}({pattern})")
    }
}

fn permission_arrays(
    effective: &Map<String, Value>,
    target: &str,
) -> (Vec<Value>, Vec<Value>, Vec<Value>) {
    let mut allow = Vec::new();
    let mut ask = Vec::new();
    let mut deny = Vec::new();
    for (category, raw_rules) in effective {
        let Some(rules) = raw_rules.as_object() else {
            continue;
        };
        let Some(tool) = permission_tool_name(category, target) else {
            continue;
        };
        for (pattern, action) in rules {
            let Some(action) = action.as_str() else {
                continue;
            };
            let entry = Value::String(permission_entry(&tool, pattern));
            match action {
                "allow" => allow.push(entry),
                "ask" => ask.push(entry),
                "deny" => deny.push(entry),
                _ => {}
            }
        }
    }
    for entries in [&mut allow, &mut ask, &mut deny] {
        entries.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
    }
    (allow, ask, deny)
}
fn permission_entry_category(entry: &str, target: &str) -> Option<String> {
    let tool = entry.split_once('(').map_or(entry, |(tool, _)| tool);
    if target == "devin" {
        return match tool {
            "Read" => Some("read".into()),
            "Write" => Some("write".into()),
            "Exec" => Some("bash".into()),
            "Fetch" => Some("webfetch".into()),
            "*" => Some("*".into()),
            _ => None,
        };
    }
    [
        "*",
        "read",
        "edit",
        "write",
        "bash",
        "webfetch",
        "websearch",
        "grep",
        "glob",
        "agent",
        "notebookedit",
        "mcp",
    ]
    .into_iter()
    .find(|category| permission_tool_name(category, target).as_deref() == Some(tool))
    .map(str::to_owned)
}

fn merge_owned_permission_entries(
    existing: &Map<String, Value>,
    key: &str,
    effective: &Map<String, Value>,
    target: &str,
    mut generated: Vec<Value>,
) -> Vec<Value> {
    if let Some(values) = existing.get(key).and_then(Value::as_array) {
        generated.extend(
            values
                .iter()
                .filter(|value| {
                    value
                        .as_str()
                        .and_then(|entry| permission_entry_category(entry, target))
                        .is_none_or(|category| !effective.contains_key(&category))
                })
                .cloned(),
        );
    }
    generated.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
    generated.dedup();
    generated
}

fn permission_category_rules<'a>(
    effective: &'a Map<String, Value>,
    category: &str,
) -> impl Iterator<Item = (&'a String, &'a str)> {
    effective
        .get(category)
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|rules| {
            rules
                .iter()
                .filter_map(|(pattern, action)| action.as_str().map(|action| (pattern, action)))
        })
}

fn map_action_bool(action: &str) -> Option<Value> {
    match action {
        "allow" => Some(Value::Bool(true)),
        "deny" => Some(Value::Bool(false)),
        _ => None,
    }
}
fn insert_permission_arrays(
    result: &mut Map<String, Value>,
    key: &str,
    effective: &Map<String, Value>,
    target: &str,
) {
    let (allow, ask, deny) = permission_arrays(effective, target);
    let mut value = Map::new();
    if !allow.is_empty() {
        value.insert("allow".into(), Value::Array(allow));
    }
    if !ask.is_empty() {
        value.insert("ask".into(), Value::Array(ask));
    }
    if !deny.is_empty() {
        value.insert("deny".into(), Value::Array(deny));
    }
    result.insert(key.into(), Value::Object(value));
}
fn merge_reasonix_permission_arrays(
    result: &mut Map<String, Value>,
    key: &str,
    effective: &Map<String, Value>,
    target: &str,
) {
    let (allow, ask, deny) = permission_arrays(effective, target);
    let managed_tools = effective
        .iter()
        .filter(|(_, value)| value.is_object())
        .filter_map(|(category, _)| permission_tool_name(category, target))
        .collect::<HashSet<_>>();
    let mut permission = result
        .remove(key)
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    for (name, generated) in [("allow", allow), ("ask", ask), ("deny", deny)] {
        let mut entries = permission
            .get(name)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        entries.retain(|entry| {
            entry.as_str().is_none_or(|entry| {
                !managed_tools
                    .iter()
                    .any(|tool| entry == tool || entry.starts_with(&format!("{tool}(")))
            })
        });
        entries.extend(generated);
        entries.retain(Value::is_string);
        entries.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
        entries.dedup();
        if entries.is_empty() {
            permission.remove(name);
        } else {
            permission.insert(name.into(), Value::Array(entries));
        }
    }
    result.insert(key.into(), Value::Object(permission));
}

fn render_codex_bash_rules(effective: &Map<String, Value>) -> String {
    let header = [
        "# Generated by Carabiner from .carabiner/permissions.jsonc (permission.bash)",
        "# https://developers.openai.com/codex/rules",
    ];
    let Some(rules) = effective.get("bash").and_then(Value::as_object) else {
        return format!(
            "{}\n# No bash permission rules were configured.",
            header.join("\n")
        );
    };
    let mut lines = header
        .iter()
        .map(|line| (*line).to_owned())
        .collect::<Vec<_>>();
    for (pattern, action) in rules {
        let tokens = pattern.split_whitespace().collect::<Vec<_>>();
        if tokens.is_empty() {
            continue;
        }
        let decision = match action.as_str() {
            Some("allow") => "allow",
            Some("ask") => "prompt",
            Some("deny") => "forbidden",
            _ => continue,
        };
        let serialized = tokens
            .iter()
            .filter_map(|token| serde_json::to_string(token).ok())
            .collect::<Vec<_>>()
            .join(", ");
        lines.extend([
            String::new(),
            format!("# {pattern}"),
            "prefix_rule(".into(),
            format!("    pattern = [{serialized}],"),
            format!("    decision = {decision:?},"),
            format!(
                "    justification = {:?},",
                format!("Generated from Carabiner permission.bash: {pattern}")
            ),
            ")".into(),
        ]);
    }
    if lines.len() == header.len() {
        lines.push("# No valid bash patterns were found.".into());
    }
    lines.join("\n")
}

fn augment_tool_name(category: &str) -> &str {
    match category {
        "bash" => "launch-process",
        "read" => "view",
        "edit" => "str-replace-editor",
        "write" => "save-file",
        "webfetch" => "web-fetch",
        "websearch" => "web-search",
        other => other,
    }
}

fn glob_to_shell_regex(pattern: &str) -> String {
    let mut regex = String::from("^");
    for character in pattern.chars() {
        match character {
            '*' => regex.push_str(".*"),
            '?' => regex.push('.'),
            '\\' | '^' | '$' | '.' | '|' | '+' | '(' | ')' | '{' | '}' | '[' | ']' => {
                regex.push('\\');
                regex.push(character);
            }
            other => regex.push(other),
        }
    }
    regex.push('$');
    regex
}

fn augment_permission_entries(effective: &Map<String, Value>) -> Vec<Value> {
    let mut entries = Vec::new();
    for (category, raw_rules) in effective {
        let Some(rules) = raw_rules.as_object() else {
            continue;
        };
        let tool = augment_tool_name(category);
        if tool == "launch-process" {
            for (pattern, action) in rules {
                let Some(action) = action.as_str() else {
                    continue;
                };
                let kind = match action {
                    "allow" => "allow",
                    "deny" => "deny",
                    "ask" => "ask-user",
                    _ => continue,
                };
                let mut entry = Map::new();
                entry.insert("toolName".into(), Value::String(tool.into()));
                if pattern != "*" {
                    entry.insert(
                        "shellInputRegex".into(),
                        Value::String(glob_to_shell_regex(pattern)),
                    );
                }
                entry.insert(
                    "permission".into(),
                    Value::Object(Map::from_iter([(
                        "type".into(),
                        Value::String(kind.into()),
                    )])),
                );
                entries.push(Value::Object(entry));
            }
        } else if rules.values().any(|value| value.as_str() == Some("deny")) {
            entries.push(json!({
                "toolName": tool,
                "permission": {"type": "deny"}
            }));
        } else if let Some(action) = rules.get("*").and_then(Value::as_str) {
            let kind = match action {
                "allow" => "allow",
                "deny" => "deny",
                "ask" => "ask-user",
                _ => continue,
            };
            entries.push(json!({
                "toolName": tool,
                "permission": {"type": kind}
            }));
        }
    }
    entries.sort_by(|left, right| {
        let left_regex = left.get("shellInputRegex").is_some();
        let right_regex = right.get("shellInputRegex").is_some();
        right_regex
            .cmp(&left_regex)
            .then_with(|| {
                let priority = |value: &Value| match value
                    .get("permission")
                    .and_then(|permission| permission.get("type"))
                    .and_then(Value::as_str)
                {
                    Some("deny") => 0,
                    Some("ask-user") => 1,
                    _ => 2,
                };
                priority(left).cmp(&priority(right))
            })
            .then_with(|| {
                left.get("toolName")
                    .and_then(Value::as_str)
                    .cmp(&right.get("toolName").and_then(Value::as_str))
            })
    });
    entries
}

fn render_permissions(
    model: &CanonicalModel,
    spec: &TargetSpec,
    global: bool,
    output_root: &Path,
) -> Result<Vec<GeneratedFile>> {
    let Some(source) = model.permissions.as_ref() else {
        return Ok(Vec::new());
    };
    let paths = spec.paths(global);
    let Some(raw_path) = &paths.permissions else {
        return Ok(Vec::new());
    };
    let path = resolve_json_variant(output_root, raw_path);
    let output_path = output_root.join(path.path());
    let existing = read_structured_or(&output_path, Value::Object(Map::new()))?;
    let effective = effective_permissions(source, &spec.name);
    if spec.name == "copilotcli" && effective.get("webfetch").is_none() && !output_path.exists() {
        return Ok(Vec::new());
    }
    if effective.is_empty() && !output_path.exists() && is_shared_config_for_path(&path) {
        return Ok(Vec::new());
    }
    let mut result = match existing {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    if spec.name == "takt" {
        let mut root = result;
        let provider = root
            .get("provider")
            .and_then(Value::as_str)
            .unwrap_or("claude")
            .to_owned();
        let mut profiles = match root.remove("provider_profiles") {
            Some(Value::Object(map)) => map,
            _ => Map::new(),
        };
        let mut profile = match profiles.remove(&provider) {
            Some(Value::Object(map)) => map,
            _ => Map::new(),
        };
        profile.insert(
            "default_permission_mode".into(),
            Value::String(derive_takt_permission_mode(&effective).into()),
        );
        if let Some(Value::Object(override_fields)) = source.get("takt") {
            for key in ["step_permission_overrides"] {
                if let Some(value) = override_fields.get(key) {
                    profile.insert(key.into(), value.clone());
                }
            }
            for key in [
                "provider_options",
                "network_policy",
                "filesystem_policy",
                "shell_policy",
                "workflow_command_gates",
            ] {
                if let Some(value) = override_fields.get(key) {
                    root.insert(key.into(), value.clone());
                }
            }
        }
        profiles.insert(provider, Value::Object(profile));
        root.insert("provider_profiles".into(), Value::Object(profiles));
        return Ok(vec![GeneratedFile::text(
            path.path(),
            serialize_json_or_yaml(&Value::Object(root), &path.file)?,
            Feature::Permissions,
        )]);
    }
    if spec.name == "codexcli" {
        let mut profiles = match result.remove("permissions") {
            Some(Value::Object(map)) => map,
            _ => Map::new(),
        };
        let base_profile = source
            .get("codexcli")
            .and_then(object_value)
            .and_then(|fields| get_string(fields, "base_permission_profile"))
            .unwrap_or_else(|| ":workspace".into());
        if base_profile == ":danger-full-access" {
            result.insert("default_permissions".into(), Value::String(base_profile));
            profiles.remove("carabiner");
        } else {
            let mut profile = match profiles.remove("carabiner") {
                Some(Value::Object(map)) => map,
                _ => Map::new(),
            };
            profile.insert("extends".into(), Value::String(base_profile.clone()));
            let mut filesystem =
                Map::from_iter([(String::from(":minimal"), Value::String("read".into()))]);
            let mut workspace_roots = Map::new();
            for category in ["read", "edit", "write"] {
                if let Some(rules) = effective.get(category).and_then(Value::as_object) {
                    for (pattern, action) in rules {
                        let access = if action.as_str() == Some("allow") {
                            if category == "read" {
                                "read"
                            } else {
                                "write"
                            }
                        } else {
                            "deny"
                        };
                        if pattern.starts_with('/')
                            || pattern.starts_with("~/")
                            || pattern.starts_with(':')
                        {
                            filesystem.insert(pattern.clone(), Value::String(access.into()));
                        } else {
                            workspace_roots.insert(pattern.clone(), Value::String(access.into()));
                        }
                    }
                }
            }
            if source
                .get("codexcli")
                .and_then(object_value)
                .and_then(|fields| fields.get("git_write_rules"))
                .and_then(Value::as_bool)
                != Some(false)
                && base_profile != ":read-only"
            {
                workspace_roots
                    .entry(".git/**")
                    .or_insert(Value::String("write".into()));
            }
            if workspace_roots.keys().any(|pattern| pattern.contains("**")) {
                filesystem.insert(
                    "glob_scan_max_depth".into(),
                    Value::Number(serde_json::Number::from(8)),
                );
            }
            if !workspace_roots.is_empty() {
                filesystem.insert(":workspace_roots".into(), Value::Object(workspace_roots));
            }
            profile.insert("filesystem".into(), Value::Object(filesystem));
            if let Some(rules) = effective.get("webfetch").and_then(Value::as_object) {
                let domains = rules
                    .iter()
                    .filter(|(_, action)| action.as_str() != Some("ask"))
                    .map(|(domain, action)| {
                        (
                            domain.clone(),
                            Value::String(
                                if action.as_str() == Some("allow") {
                                    "allow"
                                } else {
                                    "deny"
                                }
                                .into(),
                            ),
                        )
                    })
                    .collect::<Map<_, _>>();
                if !domains.is_empty() {
                    profile.insert(
                        "network".into(),
                        Value::Object(Map::from_iter([
                            (
                                String::from("enabled"),
                                Value::Bool(
                                    domains
                                        .values()
                                        .any(|value| value.as_str() == Some("allow")),
                                ),
                            ),
                            (String::from("domains"), Value::Object(domains)),
                        ])),
                    );
                }
            }
            profiles.insert("carabiner".into(), Value::Object(profile));
            result.insert(
                "default_permissions".into(),
                Value::String("carabiner".into()),
            );
        }
        if profiles.is_empty() {
            result.remove("permissions");
        } else {
            result.insert("permissions".into(), Value::Object(profiles));
        }
        if !result.contains_key("approval_policy") {
            result.insert("approval_policy".into(), Value::String("on-request".into()));
        }
        if !result.contains_key("approvals_reviewer") {
            result.insert(
                "approvals_reviewer".into(),
                Value::String("auto_review".into()),
            );
        }
        if let Some(Value::Object(override_fields)) = source.get("codexcli") {
            for key in [
                "approval_policy",
                "sandbox_mode",
                "sandbox_workspace_write",
                "apps",
                "approvals_reviewer",
                "tui",
            ] {
                if let Some(value) = override_fields.get(key) {
                    result.insert(key.into(), value.clone());
                }
            }
        }
        let mut output = vec![GeneratedFile::text(
            path.path(),
            serialize_json_or_yaml(&Value::Object(result), &path.file)?,
            Feature::Permissions,
        )];
        output.push(GeneratedFile::text(
            ".codex/rules/carabiner.rules",
            render_codex_bash_rules(&effective),
            Feature::Permissions,
        ));
        return Ok(output);
    }
    if spec.name == "augmentcode" {
        result.insert(
            "toolPermissions".into(),
            Value::Array(augment_permission_entries(&effective)),
        );
    } else if spec.name == "amp" {
        let mut disable = Vec::new();
        let mut entries = Vec::new();
        for (category, raw_rules) in &effective {
            if let Some(rules) = raw_rules.as_object() {
                for (pattern, action) in rules {
                    let Some(action) = action.as_str() else {
                        continue;
                    };
                    if pattern == "*" && action == "deny" {
                        disable.push(Value::String(category.clone()));
                        continue;
                    }
                    let mut entry = Map::new();
                    entry.insert("tool".into(), Value::String(category.clone()));
                    entry.insert(
                        "action".into(),
                        Value::String(if action == "deny" { "reject" } else { action }.into()),
                    );
                    if pattern != "*" {
                        entry.insert(
                            "matches".into(),
                            Value::Object(Map::from_iter([(
                                "cmd".into(),
                                Value::String(pattern.clone()),
                            )])),
                        );
                    }
                    entries.push(Value::Object(entry));
                }
            }
        }
        disable.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
        entries.sort_by(|left, right| {
            let specificity = |value: &Value| usize::from(value.get("matches").is_some());
            let action_priority = |value: &Value| match value.get("action").and_then(Value::as_str)
            {
                Some("reject") => 0,
                Some("ask") => 1,
                _ => 2,
            };
            let tool = |value: &Value| {
                value
                    .get("tool")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned()
            };
            let command = |value: &Value| {
                value
                    .get("matches")
                    .and_then(|matches| matches.get("cmd"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned()
            };
            action_priority(left)
                .cmp(&action_priority(right))
                .then_with(|| specificity(right).cmp(&specificity(left)))
                .then_with(|| tool(left).cmp(&tool(right)))
                .then_with(|| command(left).cmp(&command(right)))
        });
        result.insert("amp.tools.disable".into(), Value::Array(disable));
        if entries.is_empty() {
            result.remove("amp.permissions");
        } else {
            result.insert("amp.permissions".into(), Value::Array(entries));
        }
        if let Some(Value::Object(override_fields)) = source.get("amp") {
            for key in ["dangerouslyAllowAll", "mcpPermissions"] {
                if let Some(value) = override_fields.get(key) {
                    result.insert(format!("amp.{key}"), value.clone());
                }
            }
            if let Some(Value::Object(guarded)) = override_fields.get("guardedFiles") {
                if let Some(value) = guarded.get("allowlist") {
                    result.insert("amp.guardedFiles.allowlist".into(), value.clone());
                }
            }
        }
    } else if matches!(spec.name.as_str(), "claudecode" | "claudecode-legacy") {
        let mut permissions = match result.remove("permissions") {
            Some(Value::Object(map)) => map,
            _ => Map::new(),
        };
        let existing_permissions = permissions.clone();
        let (allow, ask, deny) = permission_arrays(&effective, &spec.name);
        let allow = merge_owned_permission_entries(
            &existing_permissions,
            "allow",
            &effective,
            &spec.name,
            allow,
        );
        let ask = merge_owned_permission_entries(
            &existing_permissions,
            "ask",
            &effective,
            &spec.name,
            ask,
        );
        let deny = merge_owned_permission_entries(
            &existing_permissions,
            "deny",
            &effective,
            &spec.name,
            deny,
        );
        if !allow.is_empty() {
            permissions.insert("allow".into(), Value::Array(allow));
        } else {
            permissions.remove("allow");
        }
        if !ask.is_empty() {
            permissions.insert("ask".into(), Value::Array(ask));
        } else {
            permissions.remove("ask");
        }
        if !deny.is_empty() {
            permissions.insert("deny".into(), Value::Array(deny));
        } else {
            permissions.remove("deny");
        }
        result.insert("permissions".into(), Value::Object(permissions));
    } else if spec.name == "cursor" {
        let mut permissions = match result.remove("permissions") {
            Some(Value::Object(map)) => map,
            _ => Map::new(),
        };
        permissions.insert(
            "allow".into(),
            cursor_permission_entries(&effective, "allow"),
        );
        permissions.insert("deny".into(), cursor_permission_entries(&effective, "deny"));
        result.insert("permissions".into(), Value::Object(permissions));
    } else if spec.name == "cline" {
        let mut allow = Vec::new();
        let mut deny = Vec::new();
        for (pattern, action) in permission_category_rules(&effective, "bash") {
            if action == "allow" {
                allow.push(Value::String(pattern.clone()));
            } else if action == "deny" || action == "ask" {
                deny.push(Value::String(pattern.clone()));
            }
        }
        allow.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
        deny.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
        result.insert("allow".into(), Value::Array(allow));
        result.insert("deny".into(), Value::Array(deny));
        let redirect = source
            .get("cline")
            .and_then(object_value)
            .and_then(|fields| fields.get("allowRedirects"))
            .cloned()
            .or_else(|| result.get("allowRedirects").cloned())
            .unwrap_or(Value::Bool(false));
        result.insert("allowRedirects".into(), redirect);
    } else if spec.name == "devin" {
        let mut permissions = match result.remove("permissions") {
            Some(Value::Object(map)) => map,
            _ => Map::new(),
        };
        let mut allow = Vec::new();
        let mut ask = Vec::new();
        let mut deny = Vec::new();
        let scope = |category: &str| match category {
            "read" => "Read".to_owned(),
            "edit" | "write" => "Write".to_owned(),
            "bash" => "Exec".to_owned(),
            "webfetch" => "Fetch".to_owned(),
            _ => category.to_owned(),
        };
        for (category, raw_rules) in &effective {
            if let Some(rules) = raw_rules.as_object() {
                for (pattern, action) in rules {
                    let Some(action) = action.as_str() else {
                        continue;
                    };
                    let name = scope(category);
                    let entry = if pattern == "*" {
                        name
                    } else {
                        format!("{name}({pattern})")
                    };
                    match action {
                        "allow" => allow.push(Value::String(entry)),
                        "ask" => ask.push(Value::String(entry)),
                        "deny" => deny.push(Value::String(entry)),
                        _ => {}
                    }
                }
            }
        }
        let existing_permissions = permissions.clone();
        let allow = merge_owned_permission_entries(
            &existing_permissions,
            "allow",
            &effective,
            "devin",
            allow,
        );
        let ask =
            merge_owned_permission_entries(&existing_permissions, "ask", &effective, "devin", ask);
        let deny = merge_owned_permission_entries(
            &existing_permissions,
            "deny",
            &effective,
            "devin",
            deny,
        );
        if !allow.is_empty() {
            permissions.insert("allow".into(), Value::Array(allow));
        } else {
            permissions.remove("allow");
        }
        if !ask.is_empty() {
            permissions.insert("ask".into(), Value::Array(ask));
        } else {
            permissions.remove("ask");
        }
        if !deny.is_empty() {
            permissions.insert("deny".into(), Value::Array(deny));
        } else {
            permissions.remove("deny");
        }
        result.insert("permissions".into(), Value::Object(permissions));
    } else if spec.name == "factorydroid" {
        if let Some(Value::Object(override_fields)) = source.get("factorydroid") {
            for (key, value) in override_fields {
                result.insert(key.clone(), value.clone());
            }
        }
        let mut allow = Vec::new();
        let mut deny = Vec::new();
        for (pattern, action) in permission_category_rules(&effective, "bash") {
            if action == "allow" {
                allow.push(Value::String(pattern.clone()));
            } else if action == "deny" {
                deny.push(Value::String(pattern.clone()));
            }
        }
        allow.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
        deny.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
        if allow.is_empty() {
            result.remove("commandAllowlist");
        } else {
            result.insert("commandAllowlist".into(), Value::Array(allow));
        }
        if deny.is_empty() {
            result.remove("commandDenylist");
        } else {
            result.insert("commandDenylist".into(), Value::Array(deny));
        }
    } else if spec.name == "kilo" {
        let mut permissions = match result.remove("permission") {
            Some(Value::Object(map)) => map,
            _ => Map::new(),
        };
        for (category, value) in &effective {
            permissions.insert(category.clone(), value.clone());
        }
        if let Some(Value::Object(kilo)) = source.get("kilo") {
            if let Some(Value::Object(overrides)) = kilo.get("permission") {
                for (key, value) in overrides {
                    permissions.insert(key.clone(), value.clone());
                }
            }
            if let Some(value) = kilo.get("sandbox") {
                result.insert("sandbox".into(), value.clone());
            }
        }
        result.insert("permission".into(), Value::Object(permissions));
    } else if matches!(spec.name.as_str(), "kiro" | "kiro-cli" | "kiro-ide") {
        let mut allowed_tools = match result.remove("allowedTools") {
            Some(Value::Array(values)) => values,
            _ => Vec::new(),
        };
        let mut tools = match result.remove("toolsSettings") {
            Some(Value::Object(map)) => map,
            _ => Map::new(),
        };
        let mut shell = match tools.remove("shell") {
            Some(Value::Object(map)) => map,
            _ => Map::new(),
        };
        let mut allowed_commands = Vec::new();
        let mut denied_commands = Vec::new();
        for (pattern, action) in permission_category_rules(&effective, "bash") {
            if action == "allow" {
                allowed_commands.push(Value::String(pattern.clone()));
            } else if action == "deny" {
                denied_commands.push(Value::String(pattern.clone()));
            }
        }
        shell.insert("allowedCommands".into(), Value::Array(allowed_commands));
        shell.insert("deniedCommands".into(), Value::Array(denied_commands));
        tools.insert("shell".into(), Value::Object(shell));
        for category in ["read", "edit", "write", "grep", "glob"] {
            if let Some(rules) = effective.get(category).and_then(Value::as_object) {
                let mut setting = Map::new();
                setting.insert(
                    "allowedPaths".into(),
                    Value::Array(
                        rules
                            .iter()
                            .filter(|(_, value)| value.as_str() == Some("allow"))
                            .map(|(pattern, _)| Value::String(pattern.clone()))
                            .collect(),
                    ),
                );
                setting.insert(
                    "deniedPaths".into(),
                    Value::Array(
                        rules
                            .iter()
                            .filter(|(_, value)| value.as_str() == Some("deny"))
                            .map(|(pattern, _)| Value::String(pattern.clone()))
                            .collect(),
                    ),
                );
                let output_category = if category == "edit" {
                    "write"
                } else {
                    category
                };
                tools.insert(output_category.into(), Value::Object(setting));
            }
        }
        if !effective.contains_key("read") {
            tools.insert(
                "read".into(),
                Value::Object(Map::from_iter([
                    ("allowedPaths".into(), Value::Array(Vec::new())),
                    ("deniedPaths".into(), Value::Array(Vec::new())),
                ])),
            );
        }
        if !effective.contains_key("edit") && !effective.contains_key("write") {
            tools.insert(
                "write".into(),
                Value::Object(Map::from_iter([
                    ("allowedPaths".into(), Value::Array(Vec::new())),
                    ("deniedPaths".into(), Value::Array(Vec::new())),
                ])),
            );
        }
        if let Some(Value::Object(override_fields)) = source.get("kiro") {
            if let Some(Value::Object(override_tools)) = override_fields.get("toolsSettings") {
                for (key, value) in override_tools {
                    tools.insert(key.clone(), value.clone());
                }
            }
        }
        let mut ordered_tools = Map::new();
        for key in ["shell", "read", "write", "grep", "glob"] {
            if let Some(value) = tools.remove(key) {
                ordered_tools.insert(key.into(), value);
            }
        }
        ordered_tools.extend(tools);
        tools = ordered_tools;
        if permission_category_rules(&effective, "webfetch").any(|(_, action)| action == "allow") {
            allowed_tools.push(Value::String("web_fetch".into()));
        }
        if permission_category_rules(&effective, "websearch").any(|(_, action)| action == "allow") {
            allowed_tools.push(Value::String("web_search".into()));
        }
        allowed_tools.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
        allowed_tools.dedup();
        result.insert("allowedTools".into(), Value::Array(allowed_tools));
        result.insert("toolsSettings".into(), Value::Object(tools));
        let mut ordered = Map::new();
        for key in ["hooks", "allowedTools", "toolsSettings"] {
            if let Some(value) = result.remove(key) {
                ordered.insert(key.into(), value);
            }
        }
        ordered.extend(result);
        result = ordered;
    } else if spec.name == "opencode" {
        let mut permission = match result.remove("permission") {
            Some(Value::Object(map)) => map,
            _ => Map::new(),
        };
        for (category, raw_rules) in &effective {
            let Some(rules) = raw_rules.as_object() else {
                continue;
            };
            let key = if category == "agent" {
                "task"
            } else {
                category
            };
            if [
                "webfetch",
                "websearch",
                "todowrite",
                "question",
                "doom_loop",
            ]
            .contains(&key)
            {
                let mut action = "allow";
                for value in rules.values().filter_map(Value::as_str) {
                    if value == "deny" {
                        action = "deny";
                    } else if value == "ask" && action == "allow" {
                        action = "ask";
                    }
                }
                permission.insert(key.into(), Value::String(action.into()));
            } else {
                permission.insert(key.into(), Value::Object(rules.clone()));
            }
        }
        if let Some(Value::Object(override_fields)) = source
            .get("opencode")
            .and_then(|value| value.get("permission"))
        {
            for (key, value) in override_fields {
                permission.insert(key.clone(), value.clone());
            }
        }
        result.insert("permission".into(), Value::Object(permission));
    } else if spec.name == "rovodev" {
        let mut tool_permissions = match result.remove("toolPermissions") {
            Some(Value::Object(map)) => map,
            _ => Map::new(),
        };
        let mut tools = match tool_permissions.remove("tools") {
            Some(Value::Object(map)) => map,
            _ => Map::new(),
        };
        let mut bash = match tool_permissions.remove("bash") {
            Some(Value::Object(map)) => map,
            _ => Map::new(),
        };
        let mut allowed_external = tool_permissions
            .remove("allowedExternalPaths")
            .and_then(|value| value.as_array().cloned())
            .unwrap_or_default();
        for (category, raw_rules) in &effective {
            let Some(rules) = raw_rules.as_object() else {
                continue;
            };
            for (pattern, action) in rules {
                let Some(action) = action.as_str() else {
                    continue;
                };
                if category == "*" && pattern == "*" {
                    tool_permissions.insert("default".into(), Value::String(action.into()));
                } else if category == "bash" {
                    if pattern == "*" {
                        bash.insert("default".into(), Value::String(action.into()));
                    } else {
                        let commands = bash
                            .entry("commands")
                            .or_insert_with(|| Value::Array(Vec::new()));
                        if let Some(values) = commands.as_array_mut() {
                            let generated = Value::Object(Map::from_iter([
                                ("command".into(), Value::String(pattern.clone())),
                                ("permission".into(), Value::String(action.into())),
                            ]));
                            if !values.iter().any(|value| value == &generated) {
                                values.push(generated);
                            }
                        }
                    }
                } else if let Some(keys) = match category.as_str() {
                    "read" => Some(
                        [
                            "open_files",
                            "expand_code_chunks",
                            "expand_folder",
                            "grep",
                            "getJiraIssue",
                            "getConfluencePage",
                        ]
                        .as_slice(),
                    ),
                    "edit" => Some(
                        [
                            "find_and_replace_code",
                            "create_file",
                            "delete_file",
                            "move_file",
                            "createTechnicalPlan",
                            "createJiraIssue",
                            "updateJiraIssue",
                            "createConfluencePage",
                            "updateConfluencePage",
                        ]
                        .as_slice(),
                    ),
                    "write" => Some(
                        [
                            "create_file",
                            "delete_file",
                            "move_file",
                            "find_and_replace_code",
                            "createTechnicalPlan",
                            "createJiraIssue",
                            "updateJiraIssue",
                            "createConfluencePage",
                            "updateConfluencePage",
                        ]
                        .as_slice(),
                    ),
                    _ => None,
                } {
                    if pattern == "*" {
                        for key in keys {
                            tools.insert((*key).into(), Value::String(action.into()));
                        }
                    } else if action == "allow"
                        && matches!(category.as_str(), "read" | "edit" | "write")
                    {
                        let entry = Value::String(pattern.clone());
                        if !allowed_external.iter().any(|value| value == &entry) {
                            allowed_external.push(entry);
                        }
                    }
                }
            }
        }
        if !bash.is_empty() {
            let bash = ordered_hook_fields(bash, &["default", "commands"]);
            tool_permissions.insert("bash".into(), Value::Object(bash));
        }
        if !allowed_external.is_empty() {
            tool_permissions.insert(
                "allowedExternalPaths".into(),
                Value::Array(allowed_external),
            );
        }
        if let Some(Value::Object(override_fields)) = source.get("rovodev") {
            for key in ["allowedExternalPaths", "default"] {
                if let Some(value) = override_fields.get(key) {
                    tool_permissions.insert(key.into(), value.clone());
                }
            }
        }
        if !tools.is_empty() {
            tool_permissions.insert("tools".into(), Value::Object(tools));
        }
        result.insert("toolPermissions".into(), Value::Object(tool_permissions));
    } else if spec.name == "grokcli" {
        let (allow, ask, deny) = permission_arrays(&effective, &spec.name);
        let mut permission = result
            .remove("permission")
            .and_then(|value| value.as_object().cloned())
            .unwrap_or_default();
        permission.insert("allow".into(), Value::Array(allow));
        permission.insert("deny".into(), Value::Array(deny));
        permission.insert("ask".into(), Value::Array(ask));
        result.insert("permission".into(), Value::Object(permission));
    } else if spec.name == "copilot" {
        let categories = source
            .get("permission")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        for (category, key) in [
            ("bash", "chat.tools.terminal.autoApprove"),
            ("edit", "chat.tools.edits.autoApprove"),
            ("webfetch", "chat.tools.urls.autoApprove"),
        ] {
            if let Some(Value::Object(rules)) = categories.get(category) {
                let mut values = Map::new();
                for (pattern, action) in rules {
                    if let Some(value) = action.as_str().and_then(map_action_bool) {
                        values.insert(pattern.clone(), value);
                    }
                }
                if values.is_empty() {
                    result.remove(key);
                } else {
                    result.insert(key.into(), Value::Object(values));
                }
            }
        }
    } else if matches!(
        spec.name.as_str(),
        "antigravity-cli" | "antigravity-ide" | "qwencode"
    ) {
        insert_permission_arrays(&mut result, "permissions", &effective, &spec.name);
    } else if spec.name == "reasonix" {
        merge_reasonix_permission_arrays(&mut result, "permissions", &effective, &spec.name);
    } else if spec.name == "goose" {
        let mut user = match result.remove("user") {
            Some(Value::Object(map)) => map,
            _ => Map::new(),
        };
        let mut always_allow = Vec::new();
        let mut ask_before = Vec::new();
        let mut never_allow = Vec::new();
        for (category, raw_rules) in &effective {
            if let Some(rules) = raw_rules.as_object() {
                let tool = match category.as_str() {
                    "bash" => "developer__shell",
                    "edit" | "write" => "developer__text_editor",
                    other => other,
                };
                for (pattern, action) in rules {
                    if pattern != "*" {
                        continue;
                    }
                    let entry = Value::String(tool.into());
                    match action.as_str() {
                        Some("allow") => always_allow.push(entry),
                        Some("ask") => ask_before.push(entry),
                        Some("deny") => never_allow.push(entry),
                        _ => {}
                    }
                }
            }
        }
        user.insert("always_allow".into(), Value::Array(always_allow));
        user.insert("ask_before".into(), Value::Array(ask_before));
        user.insert("never_allow".into(), Value::Array(never_allow));
        result.insert("user".into(), Value::Object(user));
    } else if spec.name == "hermesagent" {
        let allow = effective
            .values()
            .filter_map(Value::as_object)
            .flat_map(|rules| {
                rules
                    .iter()
                    .filter(|(_, action)| action.as_str() == Some("allow"))
                    .map(|(pattern, _)| Value::String(pattern.clone()))
            })
            .collect::<Vec<_>>();
        if !allow.is_empty() {
            result.insert("command_allowlist".into(), Value::Array(allow));
        }
        let deny = permission_category_rules(&effective, "bash")
            .filter(|(_, action)| *action == "deny")
            .map(|(pattern, _)| Value::String(pattern.clone()))
            .collect::<Vec<_>>();
        if !deny.is_empty() {
            result.insert(
                "approvals".into(),
                Value::Object(Map::from_iter([("deny".into(), Value::Array(deny))])),
            );
        }
        let web_deny = permission_category_rules(&effective, "webfetch")
            .filter(|(_, action)| *action == "deny")
            .map(|(pattern, _)| Value::String(pattern.clone()))
            .collect::<Vec<_>>();
        if !web_deny.is_empty() {
            result.insert(
                "security".into(),
                Value::Object(Map::from_iter([(
                    "website_blocklist".into(),
                    Value::Object(Map::from_iter([
                        ("enabled".into(), Value::Bool(true)),
                        ("domains".into(), Value::Array(web_deny)),
                    ])),
                )])),
            );
        }
    } else if spec.name == "junie" {
        let mut rules = match result.remove("rules") {
            Some(Value::Object(map)) => map,
            _ => Map::new(),
        };
        let groups = [
            ("bash", "executables"),
            ("edit", "fileEditing"),
            ("write", "fileEditing"),
            ("read", "readOutsideProject"),
            ("mcp", "mcpTools"),
        ];
        for (category, group) in groups {
            if let Some(raw) = effective.get(category) {
                if let Some(entries) = raw.as_object() {
                    let mut group_rules = Vec::new();
                    for (pattern, action) in entries {
                        let key = if pattern.chars().any(|c| matches!(c, '*' | '?' | '[')) {
                            "pattern"
                        } else {
                            "prefix"
                        };
                        let mut entry = Map::new();
                        entry.insert(key.into(), Value::String(pattern.clone()));
                        entry.insert(
                            "action".into(),
                            Value::String(
                                if action.as_str() == Some("deny") {
                                    "ask"
                                } else {
                                    action.as_str().unwrap_or("ask")
                                }
                                .into(),
                            ),
                        );
                        group_rules.push(Value::Object(entry));
                    }
                    rules.insert(
                        group.into(),
                        Value::Object(Map::from_iter([(
                            "rules".into(),
                            Value::Array(group_rules),
                        )])),
                    );
                }
            }
        }
        if let Some(Value::Object(override_fields)) = source.get("junie") {
            for key in [
                "defaultBehavior",
                "allowReadonlyCommands",
                "readSecretFile",
                "ruleDefaults",
            ] {
                if let Some(value) = override_fields.get(key) {
                    result.insert(key.into(), value.clone());
                }
            }
        }
        result.insert("rules".into(), Value::Object(rules));
    } else if spec.name == "kimi-code" {
        let mut rules = Vec::new();
        for (category, raw_rules) in &effective {
            if let Some(entries) = raw_rules.as_object() {
                let tool = match category.as_str() {
                    "bash" => "Bash",
                    "read" => "Read",
                    "edit" => "Edit",
                    "write" => "Write",
                    "grep" => "Grep",
                    "glob" => "Glob",
                    "webfetch" => "FetchURL",
                    "websearch" => "WebSearch",
                    "agent" => "Agent",
                    other => other,
                };
                for (pattern, action) in entries {
                    let mut entry = Map::new();
                    entry.insert(
                        "decision".into(),
                        Value::String(action.as_str().unwrap_or("ask").into()),
                    );
                    entry.insert(
                        "pattern".into(),
                        Value::String(if pattern == "*" {
                            tool.into()
                        } else {
                            format!("{tool}({pattern})")
                        }),
                    );
                    entry.insert("scope".into(), Value::String("user".into()));
                    rules.push(Value::Object(entry));
                }
            }
        }
        if let Some(Value::Array(entries)) =
            source.get("kimi-code").and_then(|value| value.get("rules"))
        {
            rules.extend(entries.clone());
        }
        let mut permission = match result.remove("permission") {
            Some(Value::Object(map)) => map,
            _ => Map::new(),
        };
        permission.insert("rules".into(), Value::Array(rules));
        if let Some(Value::Object(override_fields)) = source.get("kimi-code") {
            if let Some(value) = override_fields.get("defaultPermissionMode") {
                result.insert("default_permission_mode".into(), value.clone());
            }
            if let Some(value) = override_fields.get("tools") {
                result.insert("tools".into(), value.clone());
            }
        }
        result.insert("permission".into(), Value::Object(permission));
    } else if spec.name == "vibe" {
        let mut tools = match result.remove("tools") {
            Some(Value::Object(map)) => map,
            _ => Map::new(),
        };
        for (category, raw_rules) in &effective {
            let Some(rules) = raw_rules.as_object() else {
                continue;
            };
            let tool_name = match category.as_str() {
                "bash" => "bash",
                "read" => "read_file",
                "edit" => "edit",
                "write" => "write_file",
                "webfetch" => "web_fetch",
                "websearch" => "web_search",
                "grep" => "grep",
                "agent" => "task",
                _ => continue,
            };
            let mut tool = match tools.remove(tool_name) {
                Some(Value::Object(map)) => map,
                _ => Map::new(),
            };
            let mut allow = match tool.remove("allowlist") {
                Some(Value::Array(values)) => values,
                _ => Vec::new(),
            };
            let mut deny = match tool.remove("denylist") {
                Some(Value::Array(values)) => values,
                _ => Vec::new(),
            };
            for (pattern, action) in rules {
                if pattern == "*" {
                    let value = match action.as_str() {
                        Some("allow") => "always",
                        Some("deny") => "never",
                        _ => "ask",
                    };
                    tool.insert("permission".into(), Value::String(value.into()));
                } else if action.as_str() == Some("allow") {
                    allow.push(Value::String(pattern.clone()));
                } else if action.as_str() == Some("deny") {
                    deny.push(Value::String(pattern.clone()));
                }
            }
            allow.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
            allow.dedup();
            deny.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
            deny.dedup();
            if !allow.is_empty() {
                tool.insert("allowlist".into(), Value::Array(allow));
            }
            if !deny.is_empty() {
                tool.insert("denylist".into(), Value::Array(deny));
            }
            tools.insert(tool_name.into(), Value::Object(tool));
        }
        if let Some(Value::Object(override_fields)) = source.get("vibe") {
            if let Some(Value::Object(override_permission)) = override_fields.get("permission") {
                for (category, value) in override_permission {
                    let tool_name = match category.as_str() {
                        "bash" => "bash",
                        "read" => "read_file",
                        "edit" => "edit",
                        "write" => "write_file",
                        "webfetch" => "web_fetch",
                        "websearch" => "web_search",
                        "grep" => "grep",
                        "agent" => "task",
                        _ => continue,
                    };
                    if let Value::Object(fields) = value {
                        if let Some(patterns) = fields.get("sensitive_patterns") {
                            let mut tool = match tools.remove(tool_name) {
                                Some(Value::Object(map)) => map,
                                _ => Map::new(),
                            };
                            tool.insert("sensitive_patterns".into(), patterns.clone());
                            tools.insert(tool_name.into(), Value::Object(tool));
                        }
                    }
                }
            }
        }
        result.insert("tools".into(), Value::Object(tools));
    } else if spec.name == "warp" {
        let mut agents = match result.remove("agents") {
            Some(Value::Object(map)) => map,
            _ => Map::new(),
        };
        let mut profiles = match agents.remove("profiles") {
            Some(Value::Object(map)) => map,
            _ => Map::new(),
        };
        let mut allow = Vec::new();
        let mut deny = Vec::new();
        for (pattern, action) in permission_category_rules(&effective, "bash") {
            if action == "allow" {
                allow.push(Value::String(pattern.clone()));
            } else if action == "deny" {
                deny.push(Value::String(pattern.clone()));
            }
        }
        profiles.insert(
            "agent_mode_command_execution_allowlist".into(),
            Value::Array(allow.clone()),
        );
        profiles.insert(
            "agent_mode_command_execution_denylist".into(),
            Value::Array(deny.clone()),
        );
        if let Some(Value::Object(override_fields)) = source.get("warp") {
            for (key, value) in override_fields {
                if key != "execution_profile" {
                    profiles.insert(key.clone(), value.clone());
                }
            }
        }
        agents.insert("profiles".into(), Value::Object(profiles));
        if let Some(Value::Object(existing_profiles)) = agents.get_mut("execution_profiles") {
            if let Some(Value::Object(default)) = existing_profiles.get_mut("default") {
                default.insert("command_allowlist".into(), Value::Array(allow));
                default.insert("command_denylist".into(), Value::Array(deny));
                if let Some(Value::Object(override_fields)) = source
                    .get("warp")
                    .and_then(|value| value.get("execution_profile"))
                {
                    for (key, value) in override_fields {
                        default.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        result.insert("agents".into(), Value::Object(agents));
    } else if spec.name == "pi" {
        if let Some(Value::Object(pi)) = source.get("pi") {
            if let Some(value) = pi.get("defaultTools") {
                result.insert("defaultTools".into(), value.clone());
            }
        }
    } else if spec.name == "zed" {
        let mut agent = match result.remove("agent") {
            Some(Value::Object(map)) => map,
            _ => Map::new(),
        };
        let mut tool_permissions = match agent.remove("tool_permissions") {
            Some(Value::Object(map)) => map,
            _ => Map::new(),
        };
        let mut tools = match tool_permissions.remove("tools") {
            Some(Value::Object(map)) => map,
            _ => Map::new(),
        };
        for (category, raw_rules) in &effective {
            let Some(rules) = raw_rules.as_object() else {
                continue;
            };
            let tool_name = if let Some(rest) = category.strip_prefix("mcp__") {
                let mut parts = rest.splitn(2, "__");
                format!(
                    "mcp:{}:{}",
                    parts.next().unwrap_or(""),
                    parts.next().unwrap_or("")
                )
            } else {
                match category.as_str() {
                    "bash" => "terminal".into(),
                    "edit" => "edit_file".into(),
                    "write" => "write_file".into(),
                    "webfetch" => "fetch".into(),
                    "websearch" => "search_web".into(),
                    _ => continue,
                }
            };
            let mut tool = Map::new();
            for (pattern, action) in rules {
                let key = if pattern == "*" {
                    "default"
                } else {
                    match action.as_str() {
                        Some("allow") => "always_allow",
                        Some("deny") => "always_deny",
                        _ => "always_confirm",
                    }
                };
                if key == "default" {
                    tool.insert(
                        key.into(),
                        Value::String(
                            match action.as_str() {
                                Some("allow") => "allow",
                                Some("deny") => "deny",
                                _ => "confirm",
                            }
                            .into(),
                        ),
                    );
                } else {
                    let entry = Value::Object(Map::from_iter([
                        (String::from("pattern"), Value::String(pattern.clone())),
                        (String::from("case_sensitive"), Value::Bool(false)),
                    ]));
                    let list = tool.entry(key).or_insert_with(|| Value::Array(Vec::new()));
                    if let Some(values) = list.as_array_mut() {
                        values.push(entry);
                    }
                }
            }
            tools.insert(
                tool_name,
                Value::Object(ordered_hook_fields(
                    tool,
                    &["default", "always_allow", "always_confirm", "always_deny"],
                )),
            );
        }
        tool_permissions.insert("tools".into(), Value::Object(tools));
        agent.insert("tool_permissions".into(), Value::Object(tool_permissions));
        if let Some(Value::Object(zed)) = source.get("zed") {
            for key in ["sandbox_permissions", "profiles"] {
                if let Some(value) = zed.get(key) {
                    agent.insert(key.into(), value.clone());
                }
            }
        }
        result.insert("agent".into(), Value::Object(agent));
    } else if spec.name == "copilotcli" {
        if let Some(Value::Object(rules)) = source
            .get("permission")
            .and_then(|value| value.get("webfetch"))
        {
            let mut allowed = Vec::new();
            let mut denied = Vec::new();
            for (pattern, action) in rules {
                if action.as_str() == Some("allow") && global {
                    allowed.push(Value::String(pattern.clone()));
                } else if action.as_str() == Some("deny") {
                    denied.push(Value::String(pattern.clone()));
                }
            }
            if global {
                result.insert("allowedUrls".into(), Value::Array(allowed));
            }
            result.insert("deniedUrls".into(), Value::Array(denied));
        }
    } else if spec.name == "zoocode" {
        let mut allow = Vec::new();
        let mut deny = Vec::new();
        for (pattern, action) in permission_category_rules(&effective, "bash") {
            if action == "allow" {
                allow.push(Value::String(pattern.clone()));
            } else if action == "deny" {
                deny.push(Value::String(pattern.clone()));
            }
        }
        result.insert("zoo-code.allowedCommands".into(), Value::Array(allow));
        result.insert("zoo-code.deniedCommands".into(), Value::Array(deny));
    } else {
        result.insert("permission".into(), Value::Object(effective));
    }
    let result = if matches!(spec.name.as_str(), "claudecode" | "claudecode-legacy")
        && path.file == "settings.json"
    {
        ordered_hook_fields(result, &["permissions", "hooks"])
    } else {
        result
    };
    Ok(vec![GeneratedFile::text(
        path.path(),
        serialize_json_or_yaml(&Value::Object(result), &path.file)?,
        Feature::Permissions,
    )])
}

fn effective_permissions(source: &Value, target: &str) -> Map<String, Value> {
    let Some(root) = source.as_object() else {
        return Map::new();
    };
    let mut permission = match root.get("permission") {
        Some(Value::Object(map)) => map.clone(),
        _ => Map::new(),
    };
    let alias = if matches!(target, "kiro" | "kiro-cli" | "kiro-ide") {
        "kiro"
    } else if target == "hermesagent" {
        "hermes"
    } else {
        target
    };
    if let Some(Value::Object(block)) = root.get(alias) {
        if let Some(Value::Object(overrides)) = block.get("permission") {
            for (key, value) in overrides {
                permission.insert(key.clone(), value.clone());
            }
        }
    }
    permission
}

fn derive_takt_permission_mode(permissions: &Map<String, Value>) -> &'static str {
    let mut has_deny = false;
    let mut has_edit = false;
    let mut has_bash = false;
    for (category, value) in permissions {
        if let Some(rules) = value.as_object() {
            for action in rules.values().filter_map(Value::as_str) {
                has_deny |= action == "deny";
                has_edit |= (category == "edit" || category == "write") && action == "allow";
                has_bash |= category == "bash" && action == "allow";
            }
        }
    }
    if has_deny {
        "readonly"
    } else if has_edit {
        "edit"
    } else if has_bash {
        "full"
    } else {
        "readonly"
    }
}
fn permission_category(category: &str) -> &str {
    match category {
        "bash" => "Bash",
        "edit" => "Edit",
        "read" => "Read",
        "write" => "Write",
        "webfetch" => "WebFetch",
        other => other,
    }
}

fn cursor_permission_entries(permissions: &Map<String, Value>, wanted: &str) -> Value {
    let mut result = Vec::new();
    for (category, rules) in permissions {
        let Some(map) = rules.as_object() else {
            continue;
        };
        for (pattern, action) in map {
            if action.as_str() != Some(wanted) {
                continue;
            }
            let (tool, pattern) = if let Some(remainder) = category.strip_prefix("mcp__") {
                let mut parts = remainder.splitn(2, "__");
                let server = parts.next().unwrap_or("*");
                let tool = parts.next().unwrap_or("*");
                (
                    "Mcp".to_owned(),
                    format!("{server}:{tool}{}", pattern_suffix(pattern)),
                )
            } else {
                let tool = match category.as_str() {
                    "bash" => "Shell",
                    "read" => "Read",
                    "edit" | "write" => "Write",
                    "webfetch" => "WebFetch",
                    "websearch" => "WebSearch",
                    other => other,
                };
                (tool.to_owned(), pattern.clone())
            };
            result.push(Value::String(if pattern == "*" || pattern.is_empty() {
                tool
            } else {
                format!("{tool}({pattern})")
            }));
        }
    }
    result.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
    result.dedup();
    Value::Array(result)
}

fn pattern_suffix(pattern: &str) -> String {
    if pattern == "*" || pattern.is_empty() {
        String::new()
    } else {
        format!("({pattern})")
    }
}

fn hermes_ignore_plugin_init() -> String {
    r####""""Carabiner-generated project ignore enforcement for Hermes."""

import json
import os
import re
from pathlib import Path

import pathspec


PROJECT_ROOT = Path(__file__).resolve().parents[3]
PATTERNS_FILE = Path(__file__).parent / "patterns.gitignore"
PROTECTED_TOOLS = {"read_file", "write_file", "patch"}


def _spec():
    if not PATTERNS_FILE.exists():
        return pathspec.PathSpec.from_lines("gitwildmatch", [])
    return pathspec.PathSpec.from_lines("gitwildmatch", PATTERNS_FILE.read_text(encoding="utf-8").splitlines())


def _relative_paths(value):
    if not value or value == "/dev/null":
        return []
    path = Path(str(value)).expanduser()
    lexical = Path(os.path.abspath(os.path.normpath(str(path if path.is_absolute() else PROJECT_ROOT / path))))
    candidates = [lexical]
    try:
        resolved = lexical.resolve()
        if resolved != lexical:
            candidates.append(resolved)
    except OSError:
        pass

    relatives = []
    for candidate in candidates:
        try:
            relative = candidate.relative_to(PROJECT_ROOT).as_posix()
        except ValueError:
            continue
        if relative not in relatives:
            relatives.append(relative)
    return relatives


def _is_ignored_path(value, matcher):
    return any(matcher.match_file(relative) for relative in _relative_paths(value))


def _project_path_exists(value):
    return any(os.path.lexists(PROJECT_ROOT / relative) for relative in _relative_paths(value))


def _patch_paths(content):
    paths = []
    for line in str(content or "").splitlines():
        match = re.match(r"^\*\*\*\s*(?:Update|Delete|Add)\s+File:\s*(.+)$", line)
        if match:
            paths.append(match.group(1).strip())
            continue
        match = re.match(r"^\*\*\*\s*Move\s+File:\s*(.+?)\s*->\s*(.+)$", line)
        if match:
            paths.extend([match.group(1).strip(), match.group(2).strip()])
            continue
        match = re.match(r"^(?:---|\+\+\+)\s+(?:a/|b/)?(.+)$", line)
        if match:
            paths.append(match.group(1).strip())
    return paths


def _tool_paths(tool_name, args):
    if tool_name in {"read_file", "write_file"}:
        return [args.get("path")]
    if tool_name == "patch":
        return [args.get("path"), *_patch_paths(args.get("patch") or args.get("content"))]
    return []


def block_ignored_file_tools(tool_name, args, **kwargs):
    del kwargs
    if tool_name not in PROTECTED_TOOLS or not isinstance(args, dict):
        return None
    ignored = []
    matcher = _spec()
    for value in _tool_paths(tool_name, args):
        relatives = _relative_paths(value)
        if any(matcher.match_file(relative) for relative in relatives):
            ignored.extend(relatives)
    if ignored:
        return {"action": "block", "message": "Blocked by Carabiner ignore patterns: " + ", ".join(sorted(set(ignored)))}
    return None


def _filter_matches_text(value, matcher):
    kept = []
    keep_group = True
    has_path = False
    for line in str(value or "").splitlines():
        is_match_line = re.match(r"^  \d+: ", line) is not None
        ambiguous_ignored_path = _is_ignored_path(line, matcher) and _project_path_exists(line)
        if not has_path or not is_match_line or ambiguous_ignored_path:
            keep_group = not _is_ignored_path(line, matcher)
            has_path = True
        if keep_group:
            kept.append(line)
    return "\n".join(kept)


def _filter_json(value, matcher, parent_key=None):
    if isinstance(value, list):
        if parent_key == "files":
            return [item for item in value if not _is_ignored_path(item, matcher)]
        return [item for item in (_filter_json(item, matcher) for item in value) if item is not None]
    if isinstance(value, dict):
        candidate = value.get("path") or value.get("file") or value.get("filename")
        if _is_ignored_path(candidate, matcher):
            return None
        filtered = {}
        for key, item in value.items():
            if key == "counts" and isinstance(item, dict):
                filtered[key] = {
                    path: count for path, count in item.items()
                    if not _is_ignored_path(path, matcher)
                }
                continue
            if key == "matches_text":
                filtered[key] = _filter_matches_text(item, matcher)
                continue
            nested = _filter_json(item, matcher, key)
            if nested is not None:
                filtered[key] = nested

        if isinstance(filtered.get("counts"), dict):
            filtered["total_count"] = sum(filtered["counts"].values())
        elif isinstance(filtered.get("files"), list):
            filtered["total_count"] = len(filtered["files"])
        elif isinstance(filtered.get("matches"), list):
            filtered["total_count"] = len(filtered["matches"])
        elif "matches_text" in filtered:
            filtered["total_count"] = sum(
                1 for line in filtered["matches_text"].splitlines()
                if re.match(r"^  \d+: ", line) is not None
            )
        return filtered
    return value


def filter_search_results(tool_name, arguments, result, **kwargs):
    del arguments, kwargs
    if tool_name != "search_files":
        return None
    matcher = _spec()
    raw_result = str(result)
    payload, separator, hint = raw_result.partition("\n\n[Hint:")
    try:
        filtered = json.dumps(_filter_json(json.loads(payload), matcher))
        return filtered + (separator + hint if separator else "")
    except (json.JSONDecodeError, TypeError):
        kept = []
        for line in raw_result.splitlines():
            candidate = line.split(":", 1)[0].strip()
            if not _is_ignored_path(candidate, matcher):
                kept.append(line)
        return "\n".join(kept)


def register(ctx):
    ctx.register_hook("pre_tool_call", block_ignored_file_tools)
    ctx.register_hook("transform_tool_result", filter_search_results)
"####.into()
}

fn hermes_checks_plugin_init() -> String {
    r####""""Carabiner-generated project verification checks for Hermes."""

import json
from pathlib import Path


CHECKS_DIR = Path(__file__).parent / "checks"


def _load_checks():
    if not CHECKS_DIR.exists():
        return []
    checks = []
    for path in sorted(CHECKS_DIR.glob("*.json")):
        try:
            check = json.loads(path.read_text(encoding="utf-8"))
        except Exception:
            continue
        if isinstance(check, dict):
            checks.append(check)
    return checks


def require_carabiner_checks(coding, attempt, changed_paths, **kwargs):
    del kwargs
    if not coding or attempt or not changed_paths:
        return None
    checks = _load_checks()
    if not checks:
        return None

    sections = ["Run the applicable Carabiner checks before finishing."]
    sections.append("Changed paths: " + ", ".join(changed_paths))
    for check in checks:
        slug = check.get("slug") or "check"
        description = check.get("description") or slug
        severity = check.get("severity")
        tools = check.get("tools") or []
        header = f"[{slug}] {description}"
        if severity:
            header += f" (severity: {severity})"
        sections.append(header)
        if tools:
            sections.append("Suggested tools: " + ", ".join(str(tool) for tool in tools))
        body = check.get("body") or ""
        if body:
            sections.append(body)
    return {"action": "continue", "message": "\n\n".join(sections)}


def register(ctx):
    ctx.register_hook("pre_verify", require_carabiner_checks)
"####
        .into()
}
fn render_ignore(
    model: &CanonicalModel,
    spec: &TargetSpec,
    global: bool,
    output_root: &Path,
    feature_options: Option<&Map<String, Value>>,
) -> Result<Vec<GeneratedFile>> {
    let Some(content) = model.ignore.as_ref() else {
        return Ok(Vec::new());
    };
    let paths = spec.paths(global);
    let Some(raw_path) = &paths.ignore else {
        return Ok(Vec::new());
    };
    let mut path = resolve_json_variant(output_root, raw_path);
    if matches!(spec.name.as_str(), "claudecode" | "claudecode-legacy") {
        if let Some(options) = feature_options {
            if let Some(mode) = options.get("fileMode") {
                let mode = mode
                    .as_str()
                    .ok_or_else(|| anyhow!("Invalid options for {} ignore feature: fileMode must be either \"shared\" or \"local\".", spec.name))?;
                if !matches!(mode, "shared" | "local") {
                    return Err(anyhow!("Invalid options for {} ignore feature: fileMode must be either \"shared\" or \"local\".", spec.name));
                }
                if mode == "local" {
                    path = crate::targets::PathSpec::new(".claude", "settings.local.json");
                }
            }
        }
    }
    let patterns = content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| Value::String(line.into()))
        .collect::<Vec<_>>();
    if spec.name == "hermesagent" {
        let plugin_dir = hermes_plugin_dir(global, "ignore");
        let mut output = vec![GeneratedFile::text(
            path.path(),
            content.trim().to_owned(),
            Feature::Ignore,
        )];
        output.push(GeneratedFile::text(
            format!("{plugin_dir}/patterns.gitignore"),
            content.clone(),
            Feature::Ignore,
        ));
        output.push(GeneratedFile::text(
            format!("{plugin_dir}/plugin.yaml"),
            "name: carabiner-ignore\nversion: \"1.0.0\"\ndescription: Enforces Carabiner ignore patterns for Hermes file tools.\n".into(),
            Feature::Ignore,
        ));
        output.push(GeneratedFile::text(
            format!("{plugin_dir}/.carabiner-owned"),
            "Generated and owned by Carabiner.\n".into(),
            Feature::Ignore,
        ));
        output.push(GeneratedFile::text(
            format!("{plugin_dir}/__init__.py"),
            hermes_ignore_plugin_init(),
            Feature::Ignore,
        ));
        return Ok(output);
    }
    if spec.name == "zed" {
        let existing = if output_root.join(path.path()).is_file() {
            read_structured_file(&output_root.join(path.path()))?
        } else {
            Value::Object(Map::new())
        };
        let mut root = match existing {
            Value::Object(map) => map,
            _ => Map::new(),
        };
        let mut managed = patterns.clone();
        managed.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
        managed.dedup();
        if managed.is_empty() {
            root.remove("private_files");
        } else {
            root.insert("private_files".into(), Value::Array(managed));
        }
        return Ok(vec![GeneratedFile::text(
            path.path(),
            serialize_json_or_yaml(&Value::Object(root), &path.file)?,
            Feature::Ignore,
        )]);
    }
    if spec.name == "reasonix" {
        let existing = if output_root.join(path.path()).is_file() {
            read_structured_file(&output_root.join(path.path()))?
        } else {
            Value::Object(Map::new())
        };
        let mut root = match existing {
            Value::Object(map) => map,
            _ => Map::new(),
        };
        let mut permissions = match root.remove("permissions") {
            Some(Value::Object(map)) => map,
            _ => Map::new(),
        };
        permissions.insert(
            "deny".into(),
            Value::Array(
                patterns
                    .iter()
                    .filter_map(|value| value.as_str())
                    .map(|pattern| Value::String(format!("Read({pattern})")))
                    .collect(),
            ),
        );
        root.insert("permissions".into(), Value::Object(permissions));
        return Ok(vec![GeneratedFile::text(
            path.path(),
            serialize_json_or_yaml(&Value::Object(root), &path.file)?,
            Feature::Ignore,
        )]);
    }
    if matches!(spec.name.as_str(), "claudecode" | "claudecode-legacy")
        && path.file == "settings.json"
    {
        let existing = if output_root.join(path.path()).is_file() {
            read_structured_file(&output_root.join(path.path()))?
        } else {
            Value::Object(Map::new())
        };
        let mut root = match existing {
            Value::Object(map) => map,
            _ => Map::new(),
        };
        let mut permissions = match root.remove("permissions") {
            Some(Value::Object(map)) => map,
            _ => Map::new(),
        };
        let mut patterns = content
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(|line| Value::String(format!("Read({line})")))
            .collect::<Vec<_>>();
        patterns.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
        permissions.insert("deny".into(), Value::Array(patterns));
        root.insert("permissions".into(), Value::Object(permissions));
        return Ok(vec![GeneratedFile::text(
            path.path(),
            serialize_json_or_yaml(&Value::Object(root), &path.file)?,
            Feature::Ignore,
        )]);
    }
    Ok(vec![GeneratedFile::text(
        path.path(),
        content.trim().to_owned(),
        Feature::Ignore,
    )])
}

fn render_hermes_checks(checks: &[&Command], global: bool) -> Result<Vec<GeneratedFile>> {
    let plugin_dir = hermes_plugin_dir(global, "checks");
    let mut output = Vec::new();
    for check in checks {
        let slug = Path::new(&check.relative_path)
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("check");
        let mut spec = Map::new();
        spec.insert("slug".into(), Value::String(slug.into()));
        if let Some(description) = get_string(&check.frontmatter, "description") {
            spec.insert("description".into(), Value::String(description));
        }
        if let Some(severity) = get_string(&check.frontmatter, "severity") {
            spec.insert("severity".into(), Value::String(severity));
        }
        if let Some(tools) = check.frontmatter.get("tools") {
            spec.insert("tools".into(), tools.clone());
        }
        spec.insert("body".into(), Value::String(check.body.clone()));
        output.push(GeneratedFile::text(
            format!("{plugin_dir}/checks/{slug}.json"),
            format!("{}\n", serde_json::to_string_pretty(&Value::Object(spec))?),
            Feature::Checks,
        ));
    }
    output.push(GeneratedFile::text(
        format!("{plugin_dir}/plugin.yaml"),
        "name: carabiner-checks\nversion: \"1.0.0\"\ndescription: Applies Carabiner checks at the Hermes verification gate.\n".into(),
        Feature::Checks,
    ));
    output.push(GeneratedFile::text(
        format!("{plugin_dir}/.carabiner-owned"),
        "Generated and owned by Carabiner.\n".into(),
        Feature::Checks,
    ));
    output.push(GeneratedFile::text(
        format!("{plugin_dir}/__init__.py"),
        hermes_checks_plugin_init(),
        Feature::Checks,
    ));
    Ok(output)
}

fn render_aggregated_checks(checks: &[&Command]) -> String {
    let mut sections = Vec::new();
    for check in checks {
        let name = Path::new(&check.relative_path)
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("check");
        let body = if check.body.trim().is_empty() {
            get_string(&check.frontmatter, "description").unwrap_or_default()
        } else {
            check.body.trim().to_owned()
        };
        let escaped = body.replace("<!-- carabiner:check:", "<!-- carabiner:literal-check:");
        sections.push(if escaped.is_empty() {
            format!("<!-- carabiner:check:{name} -->\n## {name}")
        } else {
            format!("<!-- carabiner:check:{name} -->\n## {name}\n\n{escaped}")
        });
    }
    format!("{}\n", sections.join("\n\n"))
}

fn render_checks(
    model: &CanonicalModel,
    spec: &TargetSpec,
    global: bool,
    output_root: &Path,
) -> Result<Vec<GeneratedFile>> {
    let paths = spec.paths(global);
    let Some(path) = &paths.checks else {
        return Ok(Vec::new());
    };
    let checks = model
        .checks
        .iter()
        .filter(|check| check.targeted_at(&spec.name))
        .collect::<Vec<_>>();
    if checks.is_empty() {
        return Ok(Vec::new());
    }
    if path.file == "BUGBOT.md" || path.file == ".review-agent.md" {
        return Ok(vec![GeneratedFile::text(
            path.path(),
            render_aggregated_checks(&checks),
            Feature::Checks,
        )]);
    }
    if spec.name == "hermesagent" && path.file == "__dynamic__" {
        return render_hermes_checks(&checks, global);
    }
    if path.file == "__dynamic__" {
        return Ok(checks
            .into_iter()
            .map(|check| {
                let name = Path::new(&check.relative_path)
                    .file_stem()
                    .and_then(|v| v.to_str())
                    .unwrap_or("check")
                    .to_owned();
                let mut fm = Map::new();
                fm.insert("name".into(), Value::String(name.clone()));
                if let Some(value) = check.frontmatter.get("description") {
                    fm.insert("description".into(), value.clone());
                }
                if let Some(value) = check.frontmatter.get("severity") {
                    fm.insert("severity-default".into(), value.clone());
                }
                if let Some(value) = check.frontmatter.get("tools") {
                    fm.insert("tools".into(), value.clone());
                }
                for (key, value) in &check.frontmatter {
                    if !matches!(
                        key.as_str(),
                        "targets" | "name" | "description" | "severity" | "tools"
                    ) && !all_targets().iter().any(|target| target == key)
                    {
                        fm.insert(key.clone(), value.clone());
                    }
                }
                if let Some(fields) = check.frontmatter.get("amp").and_then(object_value) {
                    for (key, value) in fields {
                        if key != "name" {
                            fm.insert(key.clone(), value.clone());
                        }
                    }
                }
                let content = if fm.is_empty() {
                    check.body.trim().to_owned()
                } else {
                    stringify_frontmatter(&check.body, &fm).unwrap_or_else(|_| check.body.clone())
                };
                GeneratedFile::text(
                    format!("{}/{}.md", path.dir, name),
                    content,
                    Feature::Checks,
                )
            })
            .collect());
    }
    if spec.name == "takt" && path.file == "config.yaml" {
        let existing =
            read_structured_or(&output_root.join(path.path()), Value::Object(Map::new()))?;
        let mut root = match existing {
            Value::Object(map) => map,
            _ => Map::new(),
        };
        let mut overrides = match root.remove("workflow_overrides") {
            Some(Value::Object(map)) => map,
            _ => Map::new(),
        };
        let mut top_level = Vec::new();
        let mut steps: Map<String, Value> = Map::new();
        let mut personas: Map<String, Value> = Map::new();
        let mut edit_only = false;
        for check in &checks {
            let stem = Path::new(&check.relative_path)
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("quality-gate")
                .to_owned();
            let takt = check.frontmatter.get("takt").and_then(object_value);
            edit_only |= takt
                .and_then(|fields| fields.get("quality_gates_edit_only"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let gate = if let Some(command) = takt
                .and_then(|fields| fields.get("command"))
                .and_then(Value::as_str)
            {
                let mut gate = Map::new();
                gate.insert("type".into(), Value::String("command".into()));
                gate.insert(
                    "name".into(),
                    Value::String(
                        takt.and_then(|fields| get_string(fields, "name"))
                            .unwrap_or(stem.clone()),
                    ),
                );
                gate.insert("command".into(), Value::String(command.into()));
                for key in ["cwd", "timeout_ms"] {
                    if let Some(value) = takt.and_then(|fields| fields.get(key)) {
                        gate.insert(key.into(), value.clone());
                    }
                }
                Value::Object(gate)
            } else {
                Value::String(if !check.body.trim().is_empty() {
                    check.body.trim().into()
                } else {
                    get_string(&check.frontmatter, "description").unwrap_or(stem.clone())
                })
            };
            let step_names = takt
                .and_then(|fields| fields.get("steps"))
                .and_then(Value::as_array)
                .map(|values| values.iter().filter_map(Value::as_str).collect::<Vec<_>>())
                .unwrap_or_default();
            let persona_names = takt
                .and_then(|fields| fields.get("personas"))
                .and_then(Value::as_array)
                .map(|values| values.iter().filter_map(Value::as_str).collect::<Vec<_>>())
                .unwrap_or_default();
            if step_names.is_empty() && persona_names.is_empty() {
                top_level.push(gate.clone());
            }
            for name in step_names {
                let entry = steps
                    .entry(name)
                    .or_insert_with(|| Value::Array(Vec::new()));
                if let Some(values) = entry.as_array_mut() {
                    values.push(gate.clone());
                }
            }
            for name in persona_names {
                let entry = personas
                    .entry(name)
                    .or_insert_with(|| Value::Array(Vec::new()));
                if let Some(values) = entry.as_array_mut() {
                    values.push(gate.clone());
                }
            }
        }
        overrides.insert("quality_gates".into(), Value::Array(top_level));
        if edit_only {
            overrides.insert("quality_gates_edit_only".into(), Value::Bool(true));
        } else {
            overrides.remove("quality_gates_edit_only");
        }
        if !steps.is_empty() {
            overrides.insert(
                "steps".into(),
                Value::Object(
                    steps
                        .into_iter()
                        .map(|(name, gates)| {
                            (
                                name,
                                Value::Object(Map::from_iter([("quality_gates".into(), gates)])),
                            )
                        })
                        .collect(),
                ),
            );
        } else {
            overrides.remove("steps");
        }
        if !personas.is_empty() {
            overrides.insert(
                "personas".into(),
                Value::Object(
                    personas
                        .into_iter()
                        .map(|(name, gates)| {
                            (
                                name,
                                Value::Object(Map::from_iter([("quality_gates".into(), gates)])),
                            )
                        })
                        .collect(),
                ),
            );
        } else {
            overrides.remove("personas");
        }
        root.insert("workflow_overrides".into(), Value::Object(overrides));
        let root = ordered_hook_fields(
            root,
            &[
                "workflow_mcp_servers",
                "workflow_overrides",
                "provider_profiles",
            ],
        );
        return Ok(vec![GeneratedFile::text(
            path.path(),
            serialize_json_or_yaml(&Value::Object(root), &path.file)?,
            Feature::Checks,
        )]);
    }
    if spec.name == "augmentcode" && path.file == "code_review_guidelines.yaml" {
        let existing =
            read_structured_or(&output_root.join(path.path()), Value::Object(Map::new()))?;
        let mut root = existing.as_object().cloned().unwrap_or_default();
        let mut areas = root
            .get("areas")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let mut generated = Map::new();
        for check in checks {
            let stem = Path::new(&check.relative_path)
                .file_stem()
                .and_then(|v| v.to_str())
                .unwrap_or("check");
            let augment = check.frontmatter.get("augmentcode").and_then(object_value);
            let area = augment
                .and_then(|fields| get_string(fields, "area"))
                .unwrap_or_else(|| check_slug(stem, "check"));
            let area_description = augment
                .and_then(|fields| get_string(fields, "areaDescription"))
                .or_else(|| get_string(&check.frontmatter, "description"))
                .unwrap_or_else(|| stem.into());
            let globs = augment
                .and_then(|fields| fields.get("globs"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_else(|| vec![Value::String("**".into())]);
            let severity = match get_string(&check.frontmatter, "severity").as_deref() {
                Some("low") => "low",
                Some("high" | "critical") => "high",
                _ => "medium",
            };
            let description = if check.body.trim().is_empty() {
                get_string(&check.frontmatter, "description").unwrap_or_else(|| stem.into())
            } else {
                check.body.trim().into()
            };
            let id = augment
                .and_then(|fields| get_string(fields, "id"))
                .unwrap_or_else(|| check_slug(stem, "check"));
            let rule = Value::Object(Map::from_iter([
                ("id".into(), Value::String(id)),
                ("description".into(), Value::String(description)),
                ("severity".into(), Value::String(severity.into())),
            ]));
            if let Some(Value::Object(area_object)) = generated.get_mut(&area) {
                if let Some(rules) = area_object.get_mut("rules").and_then(Value::as_array_mut) {
                    rules.push(rule);
                }
            } else {
                generated.insert(
                    area,
                    Value::Object(Map::from_iter([
                        ("description".into(), Value::String(area_description)),
                        ("globs".into(), Value::Array(globs)),
                        ("rules".into(), Value::Array(vec![rule])),
                    ])),
                );
            }
        }
        areas.extend(generated);
        root.insert("areas".into(), Value::Object(areas));
        return Ok(vec![GeneratedFile::text(
            path.path(),
            serialize_json_or_yaml(&Value::Object(root), &path.file)?,
            Feature::Checks,
        )]);
    }
    let body = join_bodies(checks.iter().map(|check| check.body.as_str()));
    if path.file == "BUGBOT.md" || path.file == ".review-agent.md" {
        return Ok(vec![GeneratedFile::text(
            path.path(),
            body,
            Feature::Checks,
        )]);
    }
    let mut data = Map::new();
    data.insert("checks".into(), Value::String(body));
    let _ = output_root;
    Ok(vec![GeneratedFile::text(
        path.path(),
        serialize_json_or_yaml(&Value::Object(data), &path.file)?,
        Feature::Checks,
    )])
}

fn read_structured_file(path: &Path) -> Result<Value> {
    if !path.is_file() {
        return Err(anyhow!("missing file"));
    }
    let content = fs::read_to_string(path)?;
    if path.file_name().and_then(|name| name.to_str()) == Some(".roomodes") {
        if let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(&content) {
            return Ok(serde_json::to_value(value).unwrap_or(Value::Object(Map::new())));
        }
    }
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("yaml") | Some("yml") => Ok(serde_yaml::from_str::<serde_yaml::Value>(&content)
            .map(|value| serde_json::to_value(value).unwrap_or(Value::Object(Map::new())))?),
        Some("toml") => Ok(serde_json::to_value(content.parse::<toml::Value>()?)?),
        _ => parse_jsonc(&content),
    }
}

fn empty_for_format(format: DataFormat) -> Value {
    match format {
        DataFormat::TomlCodex
        | DataFormat::TomlReasonix
        | DataFormat::TomlVibe
        | DataFormat::TomlGrok => Value::Object(Map::new()),
        _ => Value::Object(Map::new()),
    }
}

fn toml_key(key: &str) -> String {
    if !key.is_empty()
        && key
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
    {
        key.to_owned()
    } else {
        format!("\"{}\"", key.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

fn toml_string_literal(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len() + 2);
    for character in value.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character.is_control() => {
                escaped.push_str(&format!("\\u{:04X}", character as u32));
            }
            character => escaped.push(character),
        }
    }
    format!("\"{escaped}\"")
}

fn toml_inline_value(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(toml_string_literal(value)),
        Value::Bool(value) => Some(value.to_string()),
        Value::Number(value) => Some(value.to_string()),
        Value::Array(values) => {
            let values = values
                .iter()
                .map(toml_inline_value)
                .collect::<Option<Vec<_>>>()?;
            if values.is_empty() {
                Some("[]".into())
            } else {
                Some(format!("[ {} ]", values.join(", ")))
            }
        }
        Value::Object(object) => {
            let fields = object
                .iter()
                .map(|(key, value)| {
                    Some(format!("{} = {}", toml_key(key), toml_inline_value(value)?))
                })
                .collect::<Option<Vec<_>>>()?;
            Some(format!("{{ {} }}", fields.join(", ")))
        }
        Value::Null => None,
    }
}

fn toml_path(parent: Option<&str>, key: &str) -> String {
    match parent {
        Some(parent) if !parent.is_empty() => format!("{parent}.{}", toml_key(key)),
        _ => toml_key(key),
    }
}

fn toml_emit_map_body(map: &Map<String, Value>, path: Option<&str>, lines: &mut Vec<String>) {
    let normalized_map = if path == Some("mcp_servers") {
        Some(ordered_hook_fields(
            map.clone(),
            &[
                "name",
                "type",
                "transport",
                "command",
                "args",
                "url",
                "env",
                "headers",
            ],
        ))
    } else if path.is_some_and(|path| path.starts_with("mcp_servers.")) {
        Some(ordered_hook_fields(
            map.clone(),
            &[
                "type",
                "url",
                "command",
                "args",
                "env_vars",
                "enabled_tools",
                "disabled_tools",
                "cwd",
                "timeout",
                "oauth",
                "tools",
            ],
        ))
    } else if path.is_some_and(|path| path.starts_with("tools.")) {
        Some(ordered_hook_fields(
            map.clone(),
            &["permission", "allowlist", "denylist", "sensitive_patterns"],
        ))
    } else if path == Some("plugins") {
        Some(ordered_hook_fields(
            map.clone(),
            &["name", "type", "command", "args", "url", "env"],
        ))
    } else {
        None
    };
    let map = normalized_map.as_ref().unwrap_or(map);
    for (key, value) in map {
        if value.is_object()
            || value
                .as_array()
                .is_some_and(|values| values.iter().any(Value::is_object))
        {
            continue;
        }
        if let Some(value) = toml_inline_value(value) {
            lines.push(format!("{} = {value}", toml_key(key)));
        }
    }
    let mut child_keys = map
        .iter()
        .filter(|(_, value)| {
            value.is_object()
                || value
                    .as_array()
                    .is_some_and(|values| values.iter().any(Value::is_object))
        })
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();
    if path == Some("tools") {
        child_keys.sort();
    }
    for key in child_keys {
        let Some(value) = map.get(&key) else {
            continue;
        };
        if let Value::Object(object) = value {
            let child_path = toml_path(path, &key);
            let has_inline_fields = object.iter().any(|(_, value)| {
                !value.is_object()
                    && !value
                        .as_array()
                        .is_some_and(|values| values.iter().any(Value::is_object))
            });
            if has_inline_fields || object.is_empty() {
                if !lines.is_empty() {
                    lines.push(String::new());
                }
                lines.push(format!("[{child_path}]"));
            }
            toml_emit_map_body(object, Some(&child_path), lines);
        } else if let Value::Array(values) = value {
            if !values.iter().any(Value::is_object) {
                continue;
            }
            let child_path = toml_path(path, &key);
            for value in values {
                let Value::Object(object) = value else {
                    continue;
                };
                let previous_is_header = lines
                    .last()
                    .is_some_and(|line| line.starts_with('[') && line.ends_with(']'));
                if !lines.is_empty() && !previous_is_header {
                    lines.push(String::new());
                }
                lines.push(format!("[[{child_path}]]"));
                toml_emit_map_body(object, Some(&child_path), lines);
            }
        }
    }
}

fn ordered_toml_root(object: &Map<String, Value>) -> Map<String, Value> {
    let mut remaining = object.clone();
    let mut ordered = Map::new();
    for key in [
        "default_model",
        "default_permissions",
        "approval_policy",
        "approvals_reviewer",
        "model",
        "features",
        "mcp_servers",
        "permissions",
        "plugins",
        "hooks",
        "tools",
        "disabled_tools",
    ] {
        if let Some(value) = remaining.remove(key) {
            ordered.insert(key.into(), value);
        }
    }
    ordered.extend(remaining);
    ordered
}

fn toml_pretty(value: &Value) -> Result<String> {
    let object = value
        .as_object()
        .ok_or_else(|| anyhow!("TOML output must be an object"))?;
    let mut lines = Vec::new();
    let root = ordered_toml_root(object);
    toml_emit_map_body(&root, None, &mut lines);
    if lines.is_empty() {
        Ok(String::new())
    } else {
        Ok(format!("{}\n", lines.join("\n").trim_end()))
    }
}

fn serialize_structured(value: &Value, format: DataFormat) -> Result<String> {
    match format {
        DataFormat::YamlGoose | DataFormat::YamlHermes | DataFormat::YamlTakt => Ok(format!(
            "{}\n",
            indent_yaml_sequences(&serde_yaml::to_string(value)?).trim_end()
        )),
        DataFormat::TomlCodex
        | DataFormat::TomlReasonix
        | DataFormat::TomlVibe
        | DataFormat::TomlGrok => toml_pretty(value),
        _ => json_pretty(value),
    }
}

fn serialize_json_or_yaml(value: &Value, file: &str) -> Result<String> {
    if file.ends_with(".yaml") || file.ends_with(".yml") {
        Ok(format!(
            "{}\n",
            indent_yaml_sequences(&serde_yaml::to_string(value)?).trim_end()
        ))
    } else if file.ends_with(".toml") {
        toml_pretty(value)
    } else {
        json_pretty(value)
    }
}

fn is_shared_config(format: DataFormat) -> bool {
    matches!(
        format,
        DataFormat::JsonMcpServers
            | DataFormat::JsonServers
            | DataFormat::JsonOpenCode
            | DataFormat::JsonAmp
            | DataFormat::TomlCodex
            | DataFormat::TomlGrok
            | DataFormat::TomlVibe
            | DataFormat::YamlGoose
            | DataFormat::YamlHermes
            | DataFormat::YamlTakt
    )
}
fn is_shared_config_for_path(path: &crate::targets::PathSpec) -> bool {
    matches!(
        path.file.as_str(),
        "settings.json"
            | "settings.jsonc"
            | "config.toml"
            | "config.yaml"
            | "opencode.json"
            | "opencode.jsonc"
            | "kilo.json"
            | "kilo.jsonc"
    )
}

fn check_slug(value: &str, fallback: &str) -> String {
    let slug = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '_' | '-') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_owned();
    if slug.is_empty() {
        fallback.into()
    } else {
        slug
    }
}

fn import_aggregated_checks(content: &str, fallback: &str) -> Vec<Command> {
    let mut sections = Vec::new();
    let mut preamble = Vec::new();
    let mut current: Option<(String, Vec<String>)> = None;
    for line in content.lines() {
        if let Some(marker) = line
            .trim()
            .strip_prefix("<!-- carabiner:check:")
            .and_then(|value| {
                value
                    .strip_suffix("-->")
                    .or_else(|| value.strip_suffix(" -->"))
            })
        {
            if let Some(section) = current.take() {
                sections.push(section);
            }
            current = Some((marker.trim().to_owned(), Vec::new()));
        } else if let Some((_, lines)) = current.as_mut() {
            lines.push(line.to_owned());
        } else {
            preamble.push(line.to_owned());
        }
    }
    if let Some(section) = current {
        sections.push(section);
    }
    let mut result = Vec::new();
    if !preamble.join("\n").trim().is_empty() {
        result.push(Command {
            relative_path: format!("{fallback}.md"),
            frontmatter: Map::new(),
            body: preamble.join("\n").trim().to_owned(),
        });
    }
    for (raw_name, lines) in sections {
        let name = check_slug(&raw_name, fallback);
        let mut lines = lines;
        if lines.first().map(|line| line.trim()) == Some(format!("## {raw_name}").as_str()) {
            lines.remove(0);
        }
        let body = lines
            .join("\n")
            .replace("<!-- carabiner:literal-check:", "<!-- carabiner:check:")
            .trim()
            .to_owned();
        result.push(Command {
            relative_path: format!("{name}.md"),
            frontmatter: Map::new(),
            body,
        });
    }
    if result.is_empty() {
        result.push(Command {
            relative_path: format!("{fallback}.md"),
            frontmatter: Map::new(),
            body: content.trim().to_owned(),
        });
    }
    result
}

fn import_takt_gate_values(
    gates: &[Value],
    scope: Option<(&str, &str)>,
    edit_only: bool,
    model: &mut CanonicalModel,
    index: &mut usize,
) {
    for gate in gates {
        let mut frontmatter = Map::new();
        frontmatter.insert(
            "targets".into(),
            Value::Array(vec![Value::String(if gate.is_object() {
                "takt".into()
            } else {
                "*".into()
            })]),
        );
        let body = if let Some(text) = gate.as_str() {
            text.to_owned()
        } else if let Some(object) = gate.as_object() {
            let mut takt = Map::new();
            for key in ["command", "name", "cwd", "timeout_ms"] {
                if let Some(value) = object.get(key) {
                    takt.insert(key.into(), value.clone());
                }
            }
            if let Some((kind, name)) = scope {
                takt.insert(kind.into(), Value::Array(vec![Value::String(name.into())]));
            }
            if edit_only {
                takt.insert("quality_gates_edit_only".into(), Value::Bool(true));
            }
            frontmatter.insert("takt".into(), Value::Object(takt));
            String::new()
        } else {
            continue;
        };
        if !gate.is_object() {
            if let Some((kind, name)) = scope {
                frontmatter.insert(
                    "takt".into(),
                    Value::Object(Map::from_iter([
                        (kind.into(), Value::Array(vec![Value::String(name.into())])),
                        ("quality_gates_edit_only".into(), Value::Bool(edit_only)),
                    ])),
                );
            } else if edit_only {
                frontmatter.insert(
                    "takt".into(),
                    Value::Object(Map::from_iter([(
                        "quality_gates_edit_only".into(),
                        Value::Bool(true),
                    )])),
                );
            }
        }
        model.checks.push(Command {
            relative_path: format!("takt-{}.md", *index),
            frontmatter,
            body,
        });
        *index += 1;
    }
}

fn import_takt_checks(overrides: &Map<String, Value>, model: &mut CanonicalModel) {
    let edit_only = overrides
        .get("quality_gates_edit_only")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut index = 0;
    if let Some(Value::Array(gates)) = overrides.get("quality_gates") {
        import_takt_gate_values(gates, None, edit_only, model, &mut index);
    }
    for kind in ["steps", "personas"] {
        if let Some(Value::Object(scoped)) = overrides.get(kind) {
            for (name, value) in scoped {
                if let Some(gates) = value.get("quality_gates").and_then(Value::as_array) {
                    import_takt_gate_values(
                        gates,
                        Some((kind, name)),
                        edit_only,
                        model,
                        &mut index,
                    );
                }
            }
        }
    }
}

fn kimi_logical_skill_name(name: &str) -> String {
    let normalized = name.to_ascii_lowercase();
    let conformant = !normalized.is_empty()
        && normalized
            .split('-')
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_alphanumeric()));
    if conformant {
        return normalized;
    }
    let mut encoded = String::new();
    for byte in normalized.bytes() {
        if byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')'
            )
        {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    format!("kimi-{encoded}")
}

fn kimi_canonical_skill(
    native_name: &str,
    mut frontmatter: Map<String, Value>,
    body: String,
    other_files: Vec<SkillFile>,
) -> Skill {
    let description = frontmatter
        .remove("description")
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
        .filter(|value| !value.is_empty())
        .or_else(|| {
            body.lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .map(|line| line.chars().take(240).collect())
        })
        .unwrap_or_else(|| "No description provided.".into());
    let name = frontmatter
        .remove("name")
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
        .unwrap_or_else(|| native_name.to_owned());
    let disable_model_invocation = frontmatter.remove("disableModelInvocation");
    let mut canonical = Map::new();
    canonical.insert("name".into(), Value::String(name.clone()));
    canonical.insert("description".into(), Value::String(description));
    canonical.insert(
        "targets".into(),
        Value::Array(vec![Value::String("*".into())]),
    );
    if let Some(value) = disable_model_invocation {
        canonical.insert("disable-model-invocation".into(), value);
    }
    if !frontmatter.is_empty() {
        canonical.insert("kimi-code".into(), Value::Object(frontmatter));
    }
    Skill {
        name: kimi_logical_skill_name(&name),
        frontmatter: canonical,
        body,
        other_files,
    }
}

fn load_kimi_skills(roots: &[(PathBuf, PathBuf)]) -> Result<Vec<Skill>> {
    let mut result = Vec::new();
    let mut identities = HashSet::new();
    for (root_output, root) in roots {
        let base = root_output.join(root);
        if !base.is_dir() {
            continue;
        }
        let directories = direct_dirs(&base);
        let directory_stems = directories
            .iter()
            .filter_map(|path| path.file_name().and_then(|value| value.to_str()))
            .map(ToOwned::to_owned)
            .collect::<HashSet<String>>();
        for directory in directories {
            let Some(dir_name) = directory.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            let main = directory.join("SKILL.md");
            if !main.is_file() {
                continue;
            }
            let parsed = parse_frontmatter(&fs::read_to_string(&main)?, &main)?;
            let native_name = parsed
                .data
                .get("name")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| dir_name.to_owned());
            let identity = native_name.to_ascii_lowercase();
            if !identities.insert(identity) {
                continue;
            }
            let mut other_files = Vec::new();
            for file in walk_files_following_links(&directory) {
                if file == main {
                    continue;
                }
                other_files.push(SkillFile {
                    relative_path: relative_slash(&directory, &file),
                    content: fs::read(&file)?,
                });
            }
            result.push(kimi_canonical_skill(
                &native_name,
                parsed.data,
                parsed.body,
                other_files,
            ));
        }
        let mut flat_files = walk_files_following_links(&base)
            .into_iter()
            .filter(|path| path.parent() == Some(base.as_path()))
            .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("md"))
            .collect::<Vec<_>>();
        flat_files.sort();
        for file in flat_files {
            let stem = file
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("skill");
            if directory_stems.contains(stem) {
                continue;
            }
            let content = fs::read_to_string(&file)?;
            let parsed = parse_frontmatter(&content, &file)?;
            let native_name = parsed
                .data
                .get("name")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| stem.to_owned());
            let identity = native_name.to_ascii_lowercase();
            if !identities.insert(identity) {
                continue;
            }
            result.push(kimi_canonical_skill(
                &native_name,
                parsed.data,
                parsed.body,
                Vec::new(),
            ));
        }
    }
    Ok(result)
}

fn load_kimi_subagents(roots: &[(PathBuf, PathBuf)]) -> Result<Vec<Subagent>> {
    let mut result = Vec::new();
    let mut identities = HashSet::new();
    for (root_output, root) in roots {
        let base = root_output.join(root);
        for path in walk_files_following_links(&base) {
            if path.extension().and_then(|value| value.to_str()) != Some("md") {
                continue;
            }
            let content = fs::read_to_string(&path)?;
            let parsed = parse_frontmatter(&content, &path)?;
            let fallback = path
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("agent")
                .to_ascii_lowercase();
            let name = parsed
                .data
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(&fallback)
                .to_ascii_lowercase();
            if name.is_empty()
                || !name.split('-').all(|part| {
                    !part.is_empty()
                        && part
                            .bytes()
                            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
                })
            {
                return Err(anyhow!(
                    "Invalid Kimi Code subagent name '{name}' in {}",
                    path.display()
                ));
            }
            let description = parsed
                .data
                .get("description")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .ok_or_else(|| {
                    anyhow!(
                        "Kimi Code subagent {} requires a description",
                        path.display()
                    )
                })?;
            if !identities.insert(name.clone()) {
                continue;
            }
            let mut section = parsed.data;
            section.remove("name");
            section.remove("description");
            let mut frontmatter = Map::new();
            frontmatter.insert(
                "targets".into(),
                Value::Array(vec![Value::String("*".into())]),
            );
            frontmatter.insert("name".into(), Value::String(name.clone()));
            frontmatter.insert("description".into(), Value::String(description.to_owned()));
            if !section.is_empty() {
                frontmatter.insert("kimi-code".into(), Value::Object(section));
            }
            result.push(Subagent {
                relative_path: format!("{name}.md"),
                frontmatter,
                body: parsed.body.trim().to_owned(),
            });
        }
    }
    Ok(result)
}

fn load_model_from_tool(
    spec: &TargetSpec,
    output_root: &Path,
    global: bool,
) -> Result<CanonicalModel> {
    let paths = spec.paths(global);
    let mut model = CanonicalModel::default();
    let import_primary_root = (global
        || !matches!(
            spec.name.as_str(),
            "cline" | "kiro" | "kiro-cli" | "kiro-ide"
        ))
        && !matches!(spec.name.as_str(), "roo" | "zoocode");
    if import_primary_root {
        if let Some(root) = &paths.root_rule {
            let mut candidates = vec![output_root.join(root.path())];
            if spec.name == "pi" {
                candidates.push(output_root.join(pi_override_path(global)));
            }
            if !global && matches!(spec.name.as_str(), "claudecode" | "claudecode-legacy") {
                candidates.push(output_root.join(".claude/CLAUDE.md"));
            }
            if !global && spec.name == "rovodev" {
                candidates.push(output_root.join("AGENTS.md"));
            }
            if let Some(path) = candidates.into_iter().find(|path| path.is_file()) {
                let content = fs::read_to_string(&path)?;
                let parsed = parse_frontmatter(&content, &path)?;
                let (frontmatter, body) = if parsed.has_frontmatter {
                    (parsed.data, parsed.body)
                } else {
                    (Map::new(), content.trim().to_owned())
                };
                let mut frontmatter = frontmatter;
                frontmatter.insert("root".into(), Value::Bool(spec.name != "takt"));
                frontmatter.insert(
                    "targets".into(),
                    Value::Array(vec![Value::String("*".into())]),
                );
                if spec.name == "takt" {
                    frontmatter.insert("globs".into(), Value::Array(Vec::new()));
                } else if !matches!(spec.name.as_str(), "copilot" | "copilotcli") {
                    frontmatter
                        .entry("globs")
                        .or_insert_with(|| Value::Array(vec![Value::String("**/*".into())]));
                }
                if matches!(spec.name.as_str(), "claudecode" | "claudecode-legacy") {
                    frontmatter.insert(
                        "globs".into(),
                        Value::Array(vec![Value::String("**/*".into())]),
                    );
                }
                if spec.name == "pi"
                    && path.file_name().and_then(|value| value.to_str())
                        == Some("AGENTS.override.md")
                {
                    frontmatter.insert(
                        "pi".into(),
                        Value::Object(Map::from_iter([(
                            "contextFile".into(),
                            Value::String("override".into()),
                        )])),
                    );
                }
                let relative_path = if matches!(
                    spec.name.as_str(),
                    "claudecode" | "copilot" | "copilotcli" | "augmentcode-legacy"
                ) {
                    path.file_name()
                        .and_then(|value| value.to_str())
                        .unwrap_or("overview.md")
                        .to_owned()
                } else {
                    "overview.md".into()
                };
                model.rules.push(Rule {
                    relative_path,
                    frontmatter,
                    body,
                });
            }
        }
    }
    if spec.name == "pi" {
        let append_path = output_root.join(pi_append_path(global));
        if append_path.is_file() {
            let mut frontmatter = Map::new();
            frontmatter.insert("root".into(), Value::Bool(false));
            frontmatter.insert(
                "targets".into(),
                Value::Array(vec![Value::String("pi".into())]),
            );
            frontmatter.insert(
                "pi".into(),
                Value::Object(Map::from_iter([(
                    "systemPrompt".into(),
                    Value::String("append".into()),
                )])),
            );
            model.rules.push(Rule {
                relative_path: "APPEND_SYSTEM.md".into(),
                frontmatter,
                body: fs::read_to_string(append_path)?.trim().to_owned(),
            });
        }
    }
    if let Some(dir) = &paths.nonroot_rule_dir {
        let base = output_root.join(dir);
        for path in walk_files_following_links(&base) {
            if !matches!(
                path.extension().and_then(|value| value.to_str()),
                Some("md") | Some("mdc")
            ) {
                continue;
            }
            if paths
                .root_rule
                .as_ref()
                .map(|root| path == output_root.join(root.path()))
                .unwrap_or(false)
            {
                continue;
            }
            let content = fs::read_to_string(&path)?;
            let parsed = parse_frontmatter(&content, &path)?;
            let mut frontmatter = parsed.data;
            frontmatter.insert("root".into(), Value::Bool(false));
            frontmatter.insert(
                "targets".into(),
                Value::Array(vec![Value::String("*".into())]),
            );
            if matches!(
                spec.name.as_str(),
                "aiassistant" | "kiro" | "kiro-cli" | "kiro-ide" | "roo" | "takt" | "zoocode"
            ) {
                frontmatter
                    .entry("globs")
                    .or_insert_with(|| Value::Array(Vec::new()));
            }
            if spec.name == "cursor" {
                let always_apply = frontmatter.remove("alwaysApply");
                let globs_value = frontmatter.remove("globs");
                let mut globs = globs_value
                    .as_ref()
                    .map(|value| match value {
                        Value::String(value) => value
                            .split(',')
                            .map(|part| Value::String(part.trim().into()))
                            .filter(|value| value.as_str().is_some_and(|part| !part.is_empty()))
                            .collect::<Vec<_>>(),
                        Value::Array(values) => values.clone(),
                        _ => Vec::new(),
                    })
                    .unwrap_or_default();
                if always_apply.as_ref().and_then(Value::as_bool) == Some(true) && globs.is_empty()
                {
                    globs.push(Value::String("**/*".into()));
                }
                frontmatter.insert("globs".into(), Value::Array(globs.clone()));
                let mut cursor = Map::new();
                if let Some(always_apply) = always_apply {
                    cursor.insert("alwaysApply".into(), always_apply);
                }
                if let Some(description) = frontmatter.get("description") {
                    cursor.insert("description".into(), description.clone());
                }
                if !globs.is_empty() {
                    cursor.insert("globs".into(), Value::Array(globs));
                }
                if !cursor.is_empty() {
                    frontmatter.insert("cursor".into(), Value::Object(cursor));
                }
            }
            if matches!(spec.name.as_str(), "claudecode" | "claudecode-legacy") {
                if let Some(paths_value) = frontmatter.remove("paths") {
                    frontmatter.insert(
                        "claudecode".into(),
                        Value::Object(Map::from_iter([("paths".into(), paths_value.clone())])),
                    );
                    if let Value::Array(values) = paths_value {
                        frontmatter.insert("globs".into(), Value::Array(values));
                    }
                }
            }
            if let Some(apply_to) = frontmatter.remove("applyTo") {
                if let Some(value) = apply_to.as_str() {
                    frontmatter.insert(
                        "globs".into(),
                        Value::Array(
                            value
                                .split(',')
                                .map(|part| Value::String(part.trim().into()))
                                .collect(),
                        ),
                    );
                }
            }
            if spec.name == "qwencode" {
                if let Some(paths_value) = frontmatter.remove("paths") {
                    let globs = match paths_value {
                        Value::Array(values) => Value::Array(values),
                        Value::String(value) => Value::Array(
                            value
                                .split(',')
                                .map(|part| Value::String(part.trim().into()))
                                .filter(|value| {
                                    value
                                        .as_str()
                                        .map(|value| !value.is_empty())
                                        .unwrap_or(false)
                                })
                                .collect(),
                        ),
                        other => other,
                    };
                    frontmatter.insert("globs".into(), globs);
                }
            }
            if spec.name == "cline" {
                let always = frontmatter
                    .remove("alwaysApply")
                    .and_then(|value| value.as_bool())
                    .unwrap_or(false);
                if always {
                    frontmatter.insert(
                        "globs".into(),
                        Value::Array(vec![Value::String("**/*".into())]),
                    );
                } else if let Some(paths_value) = frontmatter.remove("paths") {
                    frontmatter.insert("globs".into(), paths_value);
                } else {
                    frontmatter.insert("globs".into(), Value::Array(Vec::new()));
                }
            }
            if spec.name == "antigravity-ide" || spec.name == "antigravity-plugin" {
                let description = frontmatter.remove("description");
                let raw_globs = frontmatter.remove("globs");
                let trigger =
                    get_string(&frontmatter, "trigger").unwrap_or_else(|| "always_on".into());
                let globs = raw_globs
                    .map(|value| match value {
                        Value::Array(values) => values,
                        Value::String(value) => value
                            .split(',')
                            .map(|part| Value::String(part.trim().into()))
                            .collect(),
                        _ => Vec::new(),
                    })
                    .unwrap_or_default();
                frontmatter.remove("root");
                frontmatter.remove("targets");
                if trigger == "always_on" {
                    frontmatter.insert(
                        "globs".into(),
                        Value::Array(vec![Value::String("**/*".into())]),
                    );
                } else if !globs.is_empty() {
                    frontmatter.insert("globs".into(), Value::Array(globs));
                }
                frontmatter.insert("antigravity".into(), Value::Object(frontmatter.clone()));
                if let Some(description) = description {
                    frontmatter.insert("description".into(), description);
                }
            }
            if spec.name == "kiro" || spec.name == "kiro-cli" || spec.name == "kiro-ide" {
                let inclusion = frontmatter.remove("inclusion");
                let pattern = frontmatter.remove("fileMatchPattern");
                let mut kiro = Map::new();
                if let Some(inclusion) = inclusion {
                    kiro.insert("inclusion".into(), inclusion);
                }
                if let Some(pattern) = pattern {
                    kiro.insert("fileMatchPattern".into(), pattern);
                }
                if !kiro.is_empty() {
                    frontmatter.insert("kiro".into(), Value::Object(kiro));
                }
                let pattern = frontmatter
                    .get("kiro")
                    .and_then(object_value)
                    .and_then(|fields| fields.get("fileMatchPattern"))
                    .cloned();
                if let Some(pattern) = pattern {
                    frontmatter.insert(
                        "globs".into(),
                        match pattern {
                            Value::Array(values) => Value::Array(values),
                            Value::String(value) => Value::Array(vec![Value::String(value)]),
                            _ => Value::Array(Vec::new()),
                        },
                    );
                }
            }
            if spec.name == "augmentcode" {
                let description = frontmatter.remove("description");
                frontmatter.remove("root");
                frontmatter.remove("targets");
                let mut augment = frontmatter;
                if let Some(description) = description.clone() {
                    augment.insert("description".into(), description);
                }
                frontmatter = Map::new();
                if let Some(description) = description {
                    frontmatter.insert("description".into(), description);
                }
                frontmatter.insert("augmentcode".into(), Value::Object(augment));
            }
            frontmatter.insert("root".into(), Value::Bool(false));
            frontmatter.insert(
                "targets".into(),
                Value::Array(vec![Value::String("*".into())]),
            );
            if spec.name == "cursor" {
                let mut ordered = Map::new();
                for key in [
                    "root",
                    "localRoot",
                    "targets",
                    "description",
                    "globs",
                    "cursor",
                ] {
                    if let Some(value) = frontmatter.remove(key) {
                        ordered.insert(key.into(), value);
                    }
                }
                ordered.extend(frontmatter);
                frontmatter = ordered;
            }
            let name = relative_slash(&base, &path)
                .replace(".instructions.md", ".md")
                .replace(".mdc", ".md");
            model.rules.push(Rule {
                relative_path: name,
                frontmatter,
                body: parsed.body,
            });
        }
    }
    if matches!(spec.name.as_str(), "roo" | "zoocode") {
        load_roo_mode_rules(&mut model, output_root, &spec.name)?;
    }
    if spec.name == "reasonix" && !global {
        load_nested_reasonix_rules(&mut model, output_root)?;
    }
    if spec.name == "takt" {
        let base = output_root.join(".takt/facets/output-contracts");
        for path in walk_files_following_links(&base)
            .into_iter()
            .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("md"))
        {
            let body = fs::read_to_string(&path)?.trim().to_owned();
            let name = path
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("contract")
                .to_owned();
            let mut frontmatter = Map::new();
            frontmatter.insert(
                "targets".into(),
                Value::Array(vec![Value::String("takt".into())]),
            );
            frontmatter.insert(
                "takt".into(),
                Value::Object(Map::from_iter([(
                    "facet".into(),
                    Value::String("output-contracts".into()),
                )])),
            );
            model.rules.push(Rule {
                relative_path: format!("{name}.md"),
                frontmatter,
                body,
            });
        }
    }
    if let Some(local) = &paths.local_rule {
        let path = output_root.join(local.path());
        if path.is_file() {
            let content = fs::read_to_string(&path)?;
            let mut frontmatter = Map::new();
            frontmatter.insert("root".into(), Value::Bool(false));
            frontmatter.insert("localRoot".into(), Value::Bool(true));
            frontmatter.insert(
                "targets".into(),
                Value::Array(vec![Value::String(spec.name.clone())]),
            );
            model.rules.push(Rule {
                relative_path: local.file.clone(),
                frontmatter,
                body: content.trim().to_owned(),
            });
        }
    }
    if !global
        && matches!(
            spec.name.as_str(),
            "agentsmd" | "kiro" | "kiro-cli" | "kiro-ide"
        )
    {
        load_nested_agents_rules(&mut model, output_root, &spec.name)?;
    }

    if !matches!(spec.name.as_str(), "devin" | "warp") {
        if let Some(dir) = &paths.command_dir {
            let base = output_root.join(dir);
            for path in walk_files_following_links(&base) {
                let file_name = path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("");
                let matches_pattern = if paths.command_ext == "prompt.md" {
                    file_name.ends_with(".prompt.md")
                } else {
                    path.extension().and_then(|value| value.to_str())
                        == Some(paths.command_ext.trim_start_matches('.'))
                };
                if !matches_pattern {
                    continue;
                }
                if spec.name == "goose" {
                    if !matches!(
                        path.extension().and_then(|value| value.to_str()),
                        Some("yaml") | Some("yml")
                    ) {
                        continue;
                    }
                    let value = read_structured_file(&path)?;
                    let mut fields = value.as_object().cloned().unwrap_or_default();
                    let body = get_string(&fields, "prompt")
                        .or_else(|| get_string(&fields, "instructions"))
                        .unwrap_or_default();
                    fields.remove("prompt");
                    fields.remove("instructions");
                    let description = fields.remove("description");
                    let mut frontmatter = Map::new();
                    frontmatter.insert(
                        "targets".into(),
                        Value::Array(vec![Value::String("goose".into())]),
                    );
                    if let Some(description) = description {
                        frontmatter.insert("description".into(), description);
                    }
                    if !fields.is_empty() {
                        frontmatter.insert("goose".into(), Value::Object(fields));
                    }
                    let name = path
                        .file_stem()
                        .and_then(|value| value.to_str())
                        .unwrap_or("command");
                    model.commands.push(Command {
                        relative_path: format!("{name}.md"),
                        frontmatter,
                        body,
                    });
                    continue;
                }
                let content = fs::read_to_string(&path)?;
                let parsed = parse_frontmatter(&content, &path)?;
                let mut frontmatter = parsed.data;
                if matches!(spec.name.as_str(), "antigravity-cli" | "antigravity-ide") {
                    let mut antigravity = Map::new();
                    for key in ["trigger", "turbo"] {
                        if let Some(value) = frontmatter.remove(key) {
                            antigravity.insert(key.into(), value);
                        }
                    }
                    if !antigravity.is_empty() {
                        frontmatter.insert("antigravity".into(), Value::Object(antigravity));
                    }
                }
                if spec.name == "codexcli" {
                    let mut codex = Map::new();
                    if let Some(value) = frontmatter.remove("argument-hint") {
                        codex.insert("argument-hint".into(), value);
                    }
                    if !codex.is_empty() {
                        frontmatter.insert("codexcli".into(), Value::Object(codex));
                    }
                }
                if spec.name == "rovodev" {
                    if let Some(description) = lookup_rovodev_prompt_description(output_root, &path)
                    {
                        frontmatter.insert("description".into(), Value::String(description));
                    }
                }
                if matches!(spec.name.as_str(), "antigravity-cli" | "antigravity-ide") {
                    frontmatter.insert(
                        "targets".into(),
                        Value::Array(vec![Value::String(spec.name.clone())]),
                    );
                } else if matches!(spec.name.as_str(), "roo" | "zoocode") {
                    let description = frontmatter.remove("description");
                    let mut roo_fields = frontmatter;
                    roo_fields.remove("targets");
                    frontmatter = Map::new();
                    frontmatter.insert(
                        "targets".into(),
                        Value::Array(vec![Value::String(spec.name.clone())]),
                    );
                    if let Some(description) = description {
                        frontmatter.insert("description".into(), description);
                    }
                    if !roo_fields.is_empty() {
                        frontmatter.insert("roo".into(), Value::Object(roo_fields));
                    }
                } else {
                    frontmatter.insert(
                        "targets".into(),
                        Value::Array(vec![Value::String("*".into())]),
                    );
                }
                if let Some(name) = tool_file_stem(&path, &paths.command_ext) {
                    model.commands.push(Command {
                        relative_path: format!("{name}.md"),
                        frontmatter,
                        body: parsed.body,
                    });
                }
            }
        }
    }

    if spec.name == "opencode" {
        if let Some((config_value, _config_path)) =
            read_opencode_config_with_path(output_root, global)
        {
            if let Some(entries) = config_value.get("command").and_then(Value::as_object) {
                for (name, raw) in entries {
                    if model
                        .commands
                        .iter()
                        .any(|command| command.relative_path == format!("{name}.md"))
                    {
                        continue;
                    }
                    let Some(entry) = raw.as_object() else {
                        continue;
                    };
                    if !valid_opencode_command_entry(entry) {
                        continue;
                    }
                    let mut frontmatter = Map::new();
                    frontmatter.insert(
                        "targets".into(),
                        Value::Array(vec![Value::String("*".into())]),
                    );
                    if let Some(description) = get_string(entry, "description") {
                        frontmatter.insert("description".into(), Value::String(description));
                    }
                    let mut opencode = Map::new();
                    for (key, value) in entry {
                        if !matches!(key.as_str(), "description" | "template") {
                            opencode.insert(key.clone(), value.clone());
                        }
                    }
                    if !opencode.is_empty() {
                        frontmatter.insert("opencode".into(), Value::Object(opencode));
                    }
                    model.commands.push(Command {
                        relative_path: format!("{name}.md"),
                        frontmatter,
                        body: get_string(entry, "template").unwrap_or_default(),
                    });
                }
            }
        }
    }
    if spec.name == "kimi-code" {
        let mut roots = Vec::new();
        if let Some(dir) = &paths.subagent_dir {
            roots.push((output_root.to_path_buf(), PathBuf::from(dir)));
        }
        let shared_output = if global && std::env::var_os("KIMI_CODE_HOME").is_some() {
            home_dir().unwrap_or_else(|_| output_root.to_path_buf())
        } else {
            output_root.to_path_buf()
        };
        roots.push((shared_output, PathBuf::from(".agents/agents")));
        model.subagents.extend(load_kimi_subagents(&roots)?);
    } else if let Some(dir) = &paths.subagent_dir {
        if paths.aggregate_subagents {
            let path = output_root.join(dir).join(".roomodes");
            if path.is_file() {
                load_aggregate_subagents(&mut model, &path, &spec.name)?;
            }
        } else {
            let base = output_root.join(dir);
            for path in walk_files_following_links(&base) {
                let file_name = path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("");
                let matches_pattern = match paths.subagent_ext.as_str() {
                    "SKILL.md" | "AGENTS.md" | "AGENT.md" => file_name == paths.subagent_ext,
                    "agent.md" => file_name.ends_with(".agent.md"),
                    "md" => path.extension().and_then(|value| value.to_str()) == Some("md"),
                    pattern => {
                        path.extension().and_then(|value| value.to_str())
                            == Some(pattern.trim_start_matches('.'))
                    }
                };
                if !matches_pattern {
                    continue;
                }
                if spec.name == "codexcli" {
                    let value = read_structured_file(&path)?;
                    let mut codex_fields = value.as_object().cloned().unwrap_or_default();
                    let name = get_string(&codex_fields, "name")
                        .or_else(|| {
                            path.file_stem()
                                .and_then(|value| value.to_str())
                                .map(ToOwned::to_owned)
                        })
                        .unwrap_or_else(|| "agent".into());
                    let description = get_string(&codex_fields, "description");
                    let body =
                        get_string(&codex_fields, "developer_instructions").unwrap_or_default();
                    codex_fields.remove("name");
                    codex_fields.remove("description");
                    codex_fields.remove("developer_instructions");
                    let mut frontmatter = Map::new();
                    frontmatter.insert("name".into(), Value::String(name.clone()));
                    frontmatter.insert(
                        "targets".into(),
                        Value::Array(vec![Value::String("codexcli".into())]),
                    );
                    if let Some(description) = description {
                        frontmatter.insert("description".into(), Value::String(description));
                    }
                    if !codex_fields.is_empty() {
                        frontmatter.insert("codexcli".into(), Value::Object(codex_fields));
                    }
                    model.subagents.push(Subagent {
                        relative_path: format!(
                            "{}.md",
                            path.file_stem()
                                .and_then(|value| value.to_str())
                                .unwrap_or("agent")
                        ),
                        frontmatter,
                        body,
                    });
                    continue;
                }
                if spec.name == "hermesagent" {
                    let value = read_structured_file(&path)?;
                    let object = value.as_object().cloned().unwrap_or_default();
                    let name = get_string(&object, "name")
                        .or_else(|| {
                            path.file_stem()
                                .and_then(|value| value.to_str())
                                .map(ToOwned::to_owned)
                        })
                        .unwrap_or_else(|| "agent".into());
                    let mut frontmatter = Map::new();
                    frontmatter.insert("name".into(), Value::String(name));
                    frontmatter.insert(
                        "targets".into(),
                        Value::Array(vec![Value::String("*".into())]),
                    );
                    if let Some(description) = get_string(&object, "description") {
                        frontmatter.insert("description".into(), Value::String(description));
                    }
                    model.subagents.push(Subagent {
                        relative_path: path
                            .file_name()
                            .and_then(|value| value.to_str())
                            .unwrap_or("agent.json")
                            .replace(".json", ".md"),
                        frontmatter,
                        body: get_string(&object, "prompt").unwrap_or_default(),
                    });
                    continue;
                }
                if spec.name == "vibe" {
                    let value = read_structured_file(&path)?;
                    let object = value.as_object().cloned().unwrap_or_default();
                    let name = get_string(&object, "display_name")
                        .or_else(|| {
                            path.file_stem()
                                .and_then(|value| value.to_str())
                                .map(ToOwned::to_owned)
                        })
                        .unwrap_or_else(|| "agent".into());
                    let description = get_string(&object, "description");
                    let prompt_id = get_string(&object, "system_prompt_id");
                    let resolved_prompt = prompt_id.as_deref().and_then(|id| {
                        let prompt = output_root.join(".vibe/prompts").join(format!("{id}.md"));
                        fs::read_to_string(prompt).ok()
                    });
                    let body = resolved_prompt
                        .clone()
                        .or_else(|| get_string(&object, "system_prompt"))
                        .unwrap_or_default();
                    let mut vibe = object.clone();
                    vibe.remove("system_prompt");
                    if resolved_prompt.is_some() {
                        vibe.remove("system_prompt_id");
                    }
                    let mut frontmatter = Map::new();
                    frontmatter.insert("name".into(), Value::String(name.clone()));
                    frontmatter.insert(
                        "targets".into(),
                        Value::Array(vec![Value::String("vibe".into())]),
                    );
                    if let Some(description) = description {
                        frontmatter.insert("description".into(), Value::String(description));
                    }
                    frontmatter.insert("vibe".into(), Value::Object(vibe));
                    model.subagents.push(Subagent {
                        relative_path: format!(
                            "{}.md",
                            path.file_stem()
                                .and_then(|value| value.to_str())
                                .unwrap_or("agent")
                        ),
                        frontmatter,
                        body,
                    });
                    continue;
                }
                if matches!(spec.name.as_str(), "kiro" | "kiro-cli") && paths.subagent_ext == "json"
                {
                    let value = read_structured_file(&path)?;
                    let mut fields = value.as_object().cloned().unwrap_or_default();
                    let fallback = path
                        .file_stem()
                        .and_then(|value| value.to_str())
                        .unwrap_or("agent")
                        .to_owned();
                    let name = get_string(&fields, "name").unwrap_or(fallback);
                    let description = fields
                        .remove("description")
                        .and_then(|value| value.as_str().map(ToOwned::to_owned));
                    let body = fields
                        .remove("prompt")
                        .and_then(|value| value.as_str().map(ToOwned::to_owned))
                        .unwrap_or_default();
                    fields.remove("name");
                    let mut frontmatter = Map::new();
                    frontmatter.insert(
                        "targets".into(),
                        Value::Array(vec![Value::String(spec.name.clone())]),
                    );
                    frontmatter.insert("name".into(), Value::String(name));
                    if let Some(description) = description {
                        frontmatter.insert("description".into(), Value::String(description));
                    }
                    if !fields.is_empty() {
                        frontmatter.insert("kiro".into(), Value::Object(fields));
                    }
                    let relative_name = relative_slash(&base, &path);
                    let relative_name = relative_name
                        .strip_suffix(".json")
                        .unwrap_or(&relative_name)
                        .to_owned();
                    model.subagents.push(Subagent {
                        relative_path: format!("{relative_name}.md"),
                        frontmatter,
                        body,
                    });
                    continue;
                }
                let content = fs::read_to_string(&path)?;
                let parsed = parse_frontmatter(&content, &path)?;
                if spec.name == "kiro-ide" {
                    let mut fields = parsed.data;
                    let relative_name = relative_slash(&base, &path);
                    let fallback = relative_name
                        .strip_suffix(".md")
                        .unwrap_or(&relative_name)
                        .to_owned();
                    let name = fields
                        .remove("name")
                        .and_then(|value| value.as_str().map(ToOwned::to_owned))
                        .unwrap_or(fallback);
                    let description = fields
                        .remove("description")
                        .and_then(|value| value.as_str().map(ToOwned::to_owned))
                        .unwrap_or_default();
                    let mut frontmatter = Map::new();
                    frontmatter.insert(
                        "targets".into(),
                        Value::Array(vec![Value::String("*".into())]),
                    );
                    frontmatter.insert("name".into(), Value::String(name));
                    frontmatter.insert("description".into(), Value::String(description));
                    if !fields.is_empty() {
                        frontmatter.insert("kiro-ide".into(), Value::Object(fields));
                    }
                    model.subagents.push(Subagent {
                        relative_path: relative_name,
                        frontmatter,
                        body: parsed.body,
                    });
                    continue;
                }
                if spec.name == "reasonix"
                    && get_string(&parsed.data, "runAs").as_deref() != Some("subagent")
                {
                    continue;
                }
                let mut frontmatter = parsed.data;
                frontmatter.insert(
                    "targets".into(),
                    Value::Array(vec![Value::String("*".into())]),
                );
                let directory_layout = matches!(
                    paths.subagent_ext.as_str(),
                    "SKILL.md" | "AGENTS.md" | "AGENT.md"
                );
                let fallback_name = if directory_layout {
                    path.parent()
                        .and_then(|value| value.file_name())
                        .and_then(|value| value.to_str())
                } else if paths.subagent_ext == "agent.md" {
                    path.file_name()
                        .and_then(|value| value.to_str())
                        .and_then(|value| value.strip_suffix(".agent.md"))
                } else {
                    path.file_stem().and_then(|value| value.to_str())
                };
                let relative_tool_path = relative_slash(&base, &path);
                let source_name = get_string(&frontmatter, "name");
                let relative_name = fallback_name.unwrap_or("agent").to_owned();
                let name = source_name.clone().unwrap_or_else(|| relative_name.clone());
                let mut tool_specific = frontmatter.clone();
                tool_specific.remove("name");
                tool_specific.remove("description");
                tool_specific.remove("targets");
                frontmatter.insert("name".into(), Value::String(name));
                let clear_tool_specific =
                    |frontmatter: &mut Map<String, Value>, fields: &Map<String, Value>| {
                        for key in fields.keys().cloned().collect::<Vec<_>>() {
                            frontmatter.remove(&key);
                        }
                    };
                if spec.name == "reasonix" {
                    tool_specific.remove("invocation");
                    tool_specific.remove("runAs");
                    frontmatter.remove("invocation");
                    frontmatter.remove("runAs");
                    clear_tool_specific(&mut frontmatter, &tool_specific);
                    if !tool_specific.is_empty() {
                        frontmatter.insert("reasonix".into(), Value::Object(tool_specific.clone()));
                    }
                }
                if spec.name == "copilot" {
                    clear_tool_specific(&mut frontmatter, &tool_specific);
                    frontmatter.insert("copilot".into(), Value::Object(tool_specific));
                } else if spec.name == "copilotcli" {
                    clear_tool_specific(&mut frontmatter, &tool_specific);
                    if let Some(source_name) = source_name {
                        tool_specific.insert("name".into(), Value::String(source_name));
                    }
                    frontmatter.insert("copilotcli".into(), Value::Object(tool_specific));
                } else if matches!(spec.name.as_str(), "kilo" | "opencode") {
                    clear_tool_specific(&mut frontmatter, &tool_specific);
                    let mode = tool_specific.remove("mode").unwrap_or_else(|| {
                        Value::String(if spec.name == "kilo" {
                            "all".into()
                        } else {
                            "subagent".into()
                        })
                    });
                    let mut section = Map::new();
                    section.insert("mode".into(), mode);
                    section.extend(tool_specific);
                    frontmatter.insert(spec.name.clone(), Value::Object(section));
                } else if matches!(
                    spec.name.as_str(),
                    "cursor" | "grokcli" | "qwencode" | "rovodev"
                ) {
                    clear_tool_specific(&mut frontmatter, &tool_specific);
                    frontmatter.insert(spec.name.clone(), Value::Object(tool_specific));
                }
                let relative_path = if directory_layout {
                    format!("{relative_name}.md")
                } else if paths.subagent_ext == "agent.md" {
                    format!(
                        "{}.md",
                        relative_tool_path
                            .strip_suffix(".agent.md")
                            .unwrap_or(&relative_tool_path)
                    )
                } else if paths.subagent_ext == "yaml" {
                    format!(
                        "{}.md",
                        relative_tool_path
                            .strip_suffix(".yaml")
                            .or_else(|| relative_tool_path.strip_suffix(".yml"))
                            .unwrap_or(&relative_tool_path)
                    )
                } else if paths.subagent_ext == "toml" {
                    format!(
                        "{}.md",
                        relative_tool_path
                            .strip_suffix(".toml")
                            .unwrap_or(&relative_tool_path)
                    )
                } else {
                    relative_tool_path
                };
                model.subagents.push(Subagent {
                    relative_path,
                    frontmatter,
                    body: parsed.body,
                });
            }
        }
    }
    if spec.name == "opencode" {
        if let Some((config_value, config_path)) =
            read_opencode_config_with_path(output_root, global)
        {
            if let Some(entries) = config_value.get("agent").and_then(Value::as_object) {
                for (name, raw) in entries {
                    if model.subagents.iter().any(|agent| agent.name() == *name) {
                        continue;
                    }
                    let Some(entry) = raw.as_object() else {
                        continue;
                    };
                    if !valid_opencode_agent_entry(entry) {
                        continue;
                    }
                    let canonical_name = get_string(entry, "name").unwrap_or_else(|| name.clone());
                    let mut frontmatter = Map::new();
                    frontmatter.insert(
                        "targets".into(),
                        Value::Array(vec![Value::String("*".into())]),
                    );
                    frontmatter.insert("name".into(), Value::String(canonical_name));
                    if let Some(description) = get_string(entry, "description") {
                        frontmatter.insert("description".into(), Value::String(description));
                    }
                    let mut opencode = Map::new();
                    opencode.insert(
                        "mode".into(),
                        entry
                            .get("mode")
                            .cloned()
                            .unwrap_or_else(|| Value::String("subagent".into())),
                    );
                    for (key, value) in entry {
                        if matches!(
                            key.as_str(),
                            "model" | "temperature" | "top_p" | "disable" | "tools" | "permission"
                        ) {
                            opencode.insert(key.clone(), value.clone());
                        }
                    }
                    for (key, value) in entry {
                        if !matches!(
                            key.as_str(),
                            "name"
                                | "description"
                                | "prompt"
                                | "mode"
                                | "model"
                                | "temperature"
                                | "top_p"
                                | "disable"
                                | "tools"
                                | "permission"
                        ) {
                            opencode.insert(key.clone(), value.clone());
                        }
                    }
                    frontmatter.insert("opencode".into(), Value::Object(opencode));
                    let body = entry
                        .get("prompt")
                        .and_then(Value::as_str)
                        .map(|prompt| resolve_opencode_file_template(prompt, &config_path))
                        .unwrap_or_default();
                    model.subagents.push(Subagent {
                        relative_path: format!("{name}.md"),
                        frontmatter,
                        body,
                    });
                }
            }
        }
    }
    if spec.name == "kimi-code" {
        let mut roots = Vec::new();
        if let Some(dir) = &paths.skill_dir {
            roots.push((output_root.to_path_buf(), PathBuf::from(dir)));
        }
        let shared_output = if global && std::env::var_os("KIMI_CODE_HOME").is_some() {
            home_dir().unwrap_or_else(|_| output_root.to_path_buf())
        } else {
            output_root.to_path_buf()
        };
        roots.push((shared_output, PathBuf::from(".agents/skills")));
        model.skills.extend(load_kimi_skills(&roots)?);
    } else {
        let mut skill_dirs = Vec::new();
        if let Some(dir) = &paths.skill_dir {
            skill_dirs.push(PathBuf::from(dir));
        }
        for alternate in match spec.name.as_str() {
            "augmentcode" | "rovodev" | "junie" | "vibe" | "kimi-code" => vec![".agents/skills"],
            "opencode" => vec![".opencode/skill"],
            "claudecode" | "claudecode-legacy" => vec![".claude/scheduled-tasks"],
            _ => Vec::new(),
        } {
            let alternate = PathBuf::from(alternate);
            if !skill_dirs.contains(&alternate) {
                skill_dirs.push(alternate);
            }
        }
        for dir in skill_dirs {
            for directory in direct_dirs(&output_root.join(dir)) {
                let Some(name) = directory
                    .file_name()
                    .and_then(|value| value.to_str())
                    .map(ToOwned::to_owned)
                else {
                    continue;
                };
                let main = directory.join("SKILL.md");
                if !main.is_file() {
                    continue;
                }
                let content = fs::read_to_string(&main)?;
                let parsed = parse_frontmatter(&content, &main)?;
                if spec.name == "reasonix"
                    && get_string(&parsed.data, "runAs").as_deref() == Some("subagent")
                {
                    continue;
                }
                let mut frontmatter = parsed.data;
                frontmatter.insert("name".into(), Value::String(name.clone()));
                frontmatter.insert(
                    "targets".into(),
                    Value::Array(vec![Value::String("*".into())]),
                );
                let mut other_files = Vec::new();
                let mut openai_sidecar = None;
                for file in walk_files_following_links(&directory) {
                    if file == main {
                        continue;
                    }
                    let relative = relative_slash(&directory, &file);
                    if spec.name == "codexcli" && relative == "agents/openai.yaml" {
                        if let Ok(content) = fs::read_to_string(&file) {
                            if let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(&content) {
                                openai_sidecar = serde_json::to_value(value).ok();
                            }
                        }
                        continue;
                    }
                    other_files.push(SkillFile {
                        relative_path: relative,
                        content: fs::read(file)?,
                    });
                }
                if spec.name == "codexcli" {
                    let mut codex = frontmatter
                        .get("codexcli")
                        .and_then(object_value)
                        .cloned()
                        .unwrap_or_default();
                    if let Some(Value::Object(metadata)) = frontmatter.remove("metadata") {
                        if let Some(short) = get_string(&metadata, "short-description") {
                            codex.insert("short-description".into(), Value::String(short));
                        }
                    }
                    if let Some(Value::Object(sidecar)) = openai_sidecar {
                        for key in ["interface", "policy", "dependencies"] {
                            if let Some(value) = sidecar.get(key) {
                                codex.insert(key.into(), value.clone());
                            }
                        }
                    }
                    if !codex.is_empty() {
                        frontmatter.insert("codexcli".into(), Value::Object(codex));
                    }
                }
                model.skills.push(Skill {
                    name,
                    frontmatter,
                    body: parsed.body,
                    other_files,
                });
            }
        }
    }
    if spec.supports(Feature::Mcp, global, true) {
        if let Some(mcp) = &paths.mcp {
            let mcp_path = resolve_json_variant(output_root, mcp);
            let path = output_root.join(mcp_path.path());
            if path.is_file() {
                model.mcp = Some(import_mcp_value(
                    &read_structured_file(&path)?,
                    paths.mcp_format,
                    &spec.name,
                ));
            }
        }
    }
    if spec.name == "kimi-code" && global {
        if let Some(config_path) = &paths.hooks {
            let config_path = resolve_json_variant(output_root, config_path);
            let config_path = output_root.join(config_path.path());
            if config_path.is_file() {
                if let Ok(config) = read_structured_file(&config_path) {
                    if let Some(mcp_defaults) = config.get("mcp").and_then(Value::as_object) {
                        let mut defaults = Map::new();
                        if let Some(value) = mcp_defaults.get("startup_timeout_ms") {
                            defaults.insert("startupTimeoutMs".into(), value.clone());
                        }
                        if let Some(value) = mcp_defaults.get("tool_timeout_ms") {
                            defaults.insert("toolTimeoutMs".into(), value.clone());
                        }
                        if !defaults.is_empty() {
                            let mut imported = model
                                .mcp
                                .take()
                                .unwrap_or_else(|| json!({"mcpServers": {}}));
                            if let Some(object) = imported.as_object_mut() {
                                object.insert("kimi-code".into(), Value::Object(defaults));
                            }
                            model.mcp = Some(imported);
                        }
                    }
                }
            }
        }
    }
    if spec.supports(Feature::Hooks, global, true) && spec.name != "cline" {
        if let Some(hooks) = &paths.hooks {
            let hooks_path = resolve_json_variant(output_root, hooks);
            let path = output_root.join(hooks_path.path());
            if path.is_file()
                && !matches!(
                    path.extension().and_then(|value| value.to_str()),
                    Some("js") | Some("ts")
                )
            {
                let value = read_structured_file(&path)?;
                model.hooks = Some(match spec.name.as_str() {
                    "kimi-code" => import_kimi_hooks_value(&value),
                    "hermesagent" => import_hermes_hooks_value(&value),
                    "vibe" => import_vibe_hooks_value(&value),
                    "reasonix" => import_reasonix_hooks_value(&value),
                    "devin" if !global => import_devin_hooks_value(&value),
                    _ => import_hooks_value(&value, &spec.name),
                });
            }
        }
    }
    if spec.supports(Feature::Permissions, global, true) {
        if let Some(permissions) = &paths.permissions {
            let permissions_path = resolve_json_variant(output_root, permissions);
            let path = output_root.join(permissions_path.path());
            if path.is_file() {
                let value = read_structured_file(&path)?;
                model.permissions = Some(if spec.name == "takt" {
                    import_takt_permissions(&value)
                } else {
                    import_permissions_value(&value, &spec.name)
                });
            } else if spec.name == "copilotcli" {
                model.permissions = Some(json!({"permission": {}}));
            }
        }
    }
    if spec.supports(Feature::Ignore, global, true) {
        if let Some(ignore) = &paths.ignore {
            let ignore_path = resolve_json_variant(output_root, ignore);
            let path = output_root.join(ignore_path.path());
            if path.is_file() {
                model.ignore = Some(import_ignore_value(
                    &read_structured_file(&path).unwrap_or(Value::Null),
                    &path,
                    &spec.name,
                )?);
            }
        }
    }
    if spec.supports(Feature::Checks, global, true) {
        if let Some(checks) = &paths.checks {
            let base = output_root.join(&checks.dir);
            if spec.name == "takt" && checks.file == "config.yaml" {
                let config_path = output_root.join(checks.path());
                if config_path.is_file() {
                    let value = read_structured_file(&config_path)?;
                    if let Some(Value::Object(overrides)) = value.get("workflow_overrides") {
                        import_takt_checks(overrides, &mut model);
                    }
                }
            } else if spec.name == "augmentcode" && checks.file == "code_review_guidelines.yaml" {
                let path = output_root.join(checks.path());
                if path.is_file() {
                    let value = read_structured_file(&path)?;
                    if let Some(Value::Object(areas)) = value.get("areas") {
                        for (area, raw_area) in areas {
                            let Some(area_object) = raw_area.as_object() else {
                                continue;
                            };
                            let area_description = get_string(area_object, "description");
                            let globs = area_object.get("globs").cloned();
                            let Some(rules) = area_object.get("rules").and_then(Value::as_array)
                            else {
                                continue;
                            };
                            for raw_rule in rules {
                                let Some(rule) = raw_rule.as_object() else {
                                    continue;
                                };
                                let Some(id) = get_string(rule, "id") else {
                                    continue;
                                };
                                let Some(description) = get_string(rule, "description") else {
                                    continue;
                                };
                                let mut augment =
                                    Map::from_iter([("area".into(), Value::String(area.clone()))]);
                                if let Some(description) = area_description.clone() {
                                    augment.insert(
                                        "areaDescription".into(),
                                        Value::String(description),
                                    );
                                }
                                if let Some(globs) = globs.clone() {
                                    augment.insert("globs".into(), globs);
                                }
                                augment.insert("id".into(), Value::String(id.clone()));
                                let mut frontmatter = Map::new();
                                frontmatter.insert(
                                    "targets".into(),
                                    Value::Array(vec![Value::String("*".into())]),
                                );
                                if let Some(severity) = get_string(rule, "severity") {
                                    frontmatter.insert("severity".into(), Value::String(severity));
                                }
                                frontmatter.insert("augmentcode".into(), Value::Object(augment));
                                model.checks.push(Command {
                                    relative_path: format!("{}.md", check_slug(&id, "check")),
                                    frontmatter,
                                    body: description,
                                });
                            }
                        }
                    }
                }
            } else if checks.file == "__dynamic__" {
                for path in walk_files_following_links(&base) {
                    if spec.name == "hermesagent" {
                        let value = read_structured_file(&path)?;
                        let object = value.as_object().cloned().unwrap_or_default();
                        let mut frontmatter = Map::new();
                        frontmatter.insert(
                            "targets".into(),
                            Value::Array(vec![Value::String("*".into())]),
                        );
                        for key in ["description", "severity", "tools"] {
                            if let Some(value) = object.get(key) {
                                frontmatter.insert(key.into(), value.clone());
                            }
                        }
                        model.checks.push(Command {
                            relative_path: format!(
                                "{}.md",
                                object
                                    .get("slug")
                                    .and_then(Value::as_str)
                                    .unwrap_or("check")
                            ),
                            frontmatter,
                            body: get_string(&object, "body").unwrap_or_default(),
                        });
                    } else {
                        let content = fs::read_to_string(&path)?;
                        let parsed = parse_frontmatter(&content, &path)?;
                        let mut frontmatter = parsed.data;
                        if spec.name == "amp" {
                            frontmatter.remove("name");
                            if let Some(value) = frontmatter.remove("severity-default") {
                                frontmatter.insert("severity".into(), value);
                            }
                        }
                        model.checks.push(Command {
                            relative_path: relative_slash(&base, &path),
                            frontmatter,
                            body: parsed.body,
                        });
                    }
                }
            } else if checks.file == "BUGBOT.md" || checks.file == ".review-agent.md" {
                let path = output_root.join(checks.path());
                if path.is_file() {
                    let fallback = if checks.file == "BUGBOT.md" {
                        "bugbot"
                    } else {
                        "review-agent"
                    };
                    for mut check in import_aggregated_checks(&fs::read_to_string(path)?, fallback)
                    {
                        check.frontmatter.insert(
                            "targets".into(),
                            Value::Array(vec![Value::String("*".into())]),
                        );
                        model.checks.push(check);
                    }
                }
            }
        }
    }
    Ok(model)
}

fn load_roo_mode_rules(model: &mut CanonicalModel, output_root: &Path, target: &str) -> Result<()> {
    let base = output_root.join(".roo");
    for path in walk_files_following_links(&base) {
        let relative = relative_slash(&base, &path);
        let Some(mode) = relative.split('/').find_map(|part| {
            let mode = part.strip_prefix("rules-")?;
            (!mode.is_empty()
                && mode
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '-'))
            .then_some(mode.to_owned())
        }) else {
            continue;
        };
        if !relative.ends_with(".md") {
            continue;
        }
        let parsed = parse_frontmatter(&fs::read_to_string(&path)?, &path)?;
        let filename = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("rule.md");
        let stem = filename.strip_suffix(".md").unwrap_or(filename);
        let imported_stem = if stem.ends_with(&format!("-{mode}")) {
            stem.to_owned()
        } else {
            format!("{stem}-{mode}")
        };
        let mut frontmatter = parsed.data;
        frontmatter.insert("root".into(), Value::Bool(false));
        frontmatter.insert(
            "targets".into(),
            Value::Array(vec![Value::String(target.into())]),
        );
        frontmatter.insert("globs".into(), Value::Array(Vec::new()));
        frontmatter.insert(
            "roo".into(),
            Value::Object(Map::from_iter([("mode".into(), Value::String(mode))])),
        );
        model.rules.push(Rule {
            relative_path: format!("{imported_stem}.md"),
            frontmatter,
            body: parsed.body,
        });
    }
    Ok(())
}

fn load_nested_reasonix_rules(model: &mut CanonicalModel, output_root: &Path) -> Result<()> {
    for path in walk_files_following_links(output_root) {
        if path.file_name().and_then(|value| value.to_str()) != Some("REASONIX.md") {
            continue;
        }
        let relative = relative_slash(output_root, &path);
        if relative == "REASONIX.md"
            || relative
                .split('/')
                .any(|part| part.is_empty() || part.starts_with('.'))
        {
            continue;
        }
        let Some(parent) = Path::new(&relative).parent() else {
            continue;
        };
        let subproject = parent.to_string_lossy().replace('\\', "/");
        let content = fs::read_to_string(&path)?;
        let parsed = parse_frontmatter(&content, &path)?;
        let mut frontmatter = parsed.data;
        frontmatter.insert("root".into(), Value::Bool(false));
        frontmatter.insert(
            "targets".into(),
            Value::Array(vec![Value::String("reasonix".into())]),
        );
        frontmatter.insert(
            "agentsmd".into(),
            Value::Object(Map::from_iter([(
                "subprojectPath".into(),
                Value::String(subproject.clone()),
            )])),
        );
        let stem = subproject.replace('/', "-");
        model.rules.push(Rule {
            relative_path: format!("{stem}-reasonix.md"),
            frontmatter,
            body: parsed.body,
        });
    }
    Ok(())
}

fn load_nested_agents_rules(
    model: &mut CanonicalModel,
    output_root: &Path,
    target: &str,
) -> Result<()> {
    for path in walk_files_following_links(output_root) {
        if path.file_name().and_then(|value| value.to_str()) != Some("AGENTS.md") {
            continue;
        }
        let relative = relative_slash(output_root, &path);
        let mut parts = relative.split('/').collect::<Vec<_>>();
        if parts.len() < 2 {
            continue;
        }
        parts.pop();
        if parts.iter().any(|part| {
            part.is_empty()
                || part.starts_with('.')
                || matches!(
                    *part,
                    "node_modules" | "target" | "vendor" | "dist" | "build"
                )
        }) {
            continue;
        }
        let subproject = parts.join("/");
        let content = fs::read_to_string(&path)?;
        let parsed = parse_frontmatter(&content, &path)?;
        let mut frontmatter = parsed.data;
        frontmatter.insert("root".into(), Value::Bool(false));
        frontmatter.insert(
            "targets".into(),
            Value::Array(vec![Value::String(if target == "agentsmd" {
                "*".into()
            } else {
                target.into()
            })]),
        );
        frontmatter.insert(
            "agentsmd".into(),
            Value::Object(Map::from_iter([(
                String::from("subprojectPath"),
                Value::String(subproject.clone()),
            )])),
        );
        let slug = subproject.replace('/', "-");
        let relative_path = if target == "agentsmd" && slug.eq_ignore_ascii_case("overview") {
            format!("{slug}-agents.md")
        } else if target == "agentsmd" {
            format!("{slug}.md")
        } else {
            format!("{slug}-kiro.md")
        };
        model.rules.push(Rule {
            relative_path,
            frontmatter,
            body: parsed.body,
        });
    }
    Ok(())
}

fn lookup_rovodev_prompt_description(output_root: &Path, path: &Path) -> Option<String> {
    let manifest = read_structured_file(&output_root.join(".rovodev/prompts.yml")).ok()?;
    let prompts = manifest.get("prompts")?.as_array()?;
    let file_name = path.file_name()?.to_str()?;
    let name = Path::new(file_name).file_stem()?.to_str()?;
    let expected = format!("prompts/{file_name}");
    prompts.iter().find_map(|entry| {
        let object = entry.as_object()?;
        let matches = object.get("name").and_then(Value::as_str) == Some(name)
            || object.get("content_file").and_then(Value::as_str) == Some(expected.as_str());
        matches.then(|| {
            object
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        })
    })
}

fn tool_file_stem(path: &Path, extension: &str) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    if extension == "prompt.md" && name.ends_with(".prompt.md") {
        return Some(name.trim_end_matches(".prompt.md").into());
    }
    path.file_stem()
        .and_then(|value| value.to_str())
        .map(ToOwned::to_owned)
}

fn load_aggregate_subagents(model: &mut CanonicalModel, path: &Path, target: &str) -> Result<()> {
    let value = read_structured_file(path)?;
    let modes = value
        .get("customModes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for mode in modes {
        let Some(object) = mode.as_object() else {
            continue;
        };
        let slug = get_string(object, "slug")
            .or_else(|| get_string(object, "name"))
            .unwrap_or_else(|| "mode".into());
        safe_name(&slug)?;
        let name = get_string(object, "name").unwrap_or_else(|| slug.clone());
        let body = get_string(object, "roleDefinition")
            .or_else(|| get_string(object, "customInstructions"))
            .unwrap_or_default();
        let mut roo_fields = object.clone();
        for key in ["slug", "name", "description", "roleDefinition"] {
            roo_fields.remove(key);
        }
        let zoocode_allowed = if target == "zoocode" {
            roo_fields.remove("allowedMcpServers")
        } else {
            None
        };
        roo_fields.insert("slug".into(), Value::String(slug.clone()));
        let mut fm = Map::new();
        fm.insert("name".into(), Value::String(name));
        fm.insert(
            "targets".into(),
            Value::Array(vec![Value::String(target.into())]),
        );
        if let Some(description) = get_string(object, "description") {
            fm.insert("description".into(), Value::String(description));
        }
        fm.insert("roo".into(), Value::Object(roo_fields));
        if let Some(allowed) = zoocode_allowed {
            fm.insert(
                "zoocode".into(),
                Value::Object(Map::from_iter([("allowedMcpServers".into(), allowed)])),
            );
        }
        model.subagents.push(Subagent {
            relative_path: format!("{slug}.md"),
            frontmatter: fm,
            body,
        });
    }
    Ok(())
}

fn prepend_schema(value: &mut Value, schema: &str) {
    let Value::Object(object) = value else {
        return;
    };
    let mut ordered = Map::new();
    ordered.insert("$schema".into(), Value::String(schema.into()));
    for (key, value) in std::mem::take(object) {
        if key != "$schema" {
            ordered.insert(key, value);
        }
    }
    *object = ordered;
}

fn global_import_feature_is_project_scoped(target: &str, feature: &str) -> bool {
    match feature {
        "rules" => matches!(
            target,
            "amp"
                | "antigravity-cli"
                | "antigravity-ide"
                | "deepagents"
                | "cursor"
                | "augmentcode"
                | "claudecode-legacy"
                | "cline"
                | "codexcli"
                | "factorydroid"
                | "goose"
                | "grokcli"
                | "junie"
                | "kilo"
                | "kiro"
                | "kiro-cli"
                | "kiro-ide"
                | "opencode"
                | "pi"
                | "qwencode"
                | "reasonix"
                | "rovodev"
                | "takt"
                | "vibe"
                | "warp"
                | "devin"
                | "zed"
        ),
        "ignore" => matches!(target, "kiro" | "kiro-cli" | "kiro-ide" | "devin" | "zed"),
        "commands" => matches!(
            target,
            "antigravity-cli"
                | "antigravity-ide"
                | "augmentcode"
                | "claudecode"
                | "claudecode-legacy"
                | "cline"
                | "codexcli"
                | "factorydroid"
                | "goose"
                | "cursor"
                | "grokcli"
                | "junie"
                | "kilo"
                | "kiro-cli"
                | "opencode"
                | "pi"
                | "qwencode"
                | "reasonix"
                | "roo"
                | "rovodev"
                | "takt"
                | "zoocode"
        ),
        "subagents" => matches!(
            target,
            "antigravity-cli"
                | "antigravity-ide"
                | "augmentcode"
                | "claudecode"
                | "claudecode-legacy"
                | "cline"
                | "cursor"
                | "codexcli"
                | "copilot"
                | "copilotcli"
                | "factorydroid"
                | "goose"
                | "grokcli"
                | "junie"
                | "kilo"
                | "kiro-cli"
                | "kiro-ide"
                | "opencode"
                | "qwencode"
                | "rovodev"
                | "takt"
        ),
        _ => false,
    }
}

fn add_import_result(total: &mut ImportResult, next: ImportResult) {
    total.rules += next.rules;
    total.ignore += next.ignore;
    total.mcp += next.mcp;
    total.commands += next.commands;
    total.subagents += next.subagents;
    total.skills += next.skills;
    total.hooks += next.hooks;
    total.permissions += next.permissions;
    total.checks += next.checks;
}

fn write_canonical_model_global(
    model: &CanonicalModel,
    project_cwd: &Path,
    canonical_home: &Path,
    features: &[String],
    target: &str,
    dry_run: bool,
    add_mcp_schema: bool,
) -> Result<ImportResult> {
    let project_features = features
        .iter()
        .filter(|feature| global_import_feature_is_project_scoped(target, feature))
        .cloned()
        .collect::<Vec<_>>();
    let home_features = features
        .iter()
        .filter(|feature| !global_import_feature_is_project_scoped(target, feature))
        .cloned()
        .collect::<Vec<_>>();
    let mut result = write_canonical_model_at(
        model,
        &project_cwd.join(".carabiner"),
        &project_features,
        dry_run,
        add_mcp_schema,
    )?;
    let home_result = write_canonical_model_at(
        model,
        &canonical_home.join(".carabiner"),
        &home_features,
        dry_run,
        add_mcp_schema,
    )?;
    add_import_result(&mut result, home_result);
    Ok(result)
}

fn write_canonical_model(
    model: &CanonicalModel,
    cwd: &Path,
    features: &[String],
    dry_run: bool,
    add_mcp_schema: bool,
) -> Result<ImportResult> {
    write_canonical_model_at(
        model,
        &cwd.join(".carabiner"),
        features,
        dry_run,
        add_mcp_schema,
    )
}

fn write_canonical_model_at(
    model: &CanonicalModel,
    output_dir: &Path,
    features: &[String],
    dry_run: bool,
    add_mcp_schema: bool,
) -> Result<ImportResult> {
    let mut result = ImportResult::default();
    let enabled = |feature: Feature| features.iter().any(|name| name == feature.as_str());
    if enabled(Feature::Rules) {
        for rule in &model.rules {
            let path = output_dir.join("rules").join(&rule.relative_path);
            if write_text(
                &path,
                &stringify_frontmatter(&rule.body, &rule.frontmatter)?,
                dry_run,
            )? {
                result.rules += 1;
            }
        }
    }
    if enabled(Feature::Commands) {
        for command in &model.commands {
            let path = output_dir.join("commands").join(&command.relative_path);
            if write_text(
                &path,
                &stringify_frontmatter(&command.body, &command.frontmatter)?,
                dry_run,
            )? {
                result.commands += 1;
            }
        }
    }
    if enabled(Feature::Subagents) {
        for agent in &model.subagents {
            let path = output_dir.join("subagents").join(&agent.relative_path);
            if write_text(
                &path,
                &stringify_frontmatter(&agent.body, &agent.frontmatter)?,
                dry_run,
            )? {
                result.subagents += 1;
            }
        }
    }
    if enabled(Feature::Skills) {
        for skill in &model.skills {
            let path = output_dir.join("skills").join(&skill.name).join("SKILL.md");
            if write_text(
                &path,
                &stringify_frontmatter(&skill.body, &skill.frontmatter)?,
                dry_run,
            )? {
                result.skills += 1;
            }
            for file in &skill.other_files {
                let path = output_dir
                    .join("skills")
                    .join(&skill.name)
                    .join(&file.relative_path);
                write_bytes(&path, &file.content, dry_run)?;
            }
        }
    }
    if enabled(Feature::Mcp) {
        if let Some(value) = &model.mcp {
            let mut value = value.clone();
            if add_mcp_schema {
                prepend_schema(
                    &mut value,
                    "https://github.com/findyourexit/carabiner/releases/latest/download/mcp-schema.json",
                );
            }
            let path = output_dir.join("mcp.jsonc");
            if write_text(&path, &json_pretty(&value)?, dry_run)? {
                result.mcp += 1;
            }
        }
    }
    if enabled(Feature::Hooks) {
        if let Some(value) = &model.hooks {
            let path = output_dir.join("hooks.jsonc");
            if write_text(&path, &json_pretty(value)?, dry_run)? {
                result.hooks += 1;
            }
        }
    }
    if enabled(Feature::Permissions) {
        if let Some(value) = &model.permissions {
            let path = output_dir.join("permissions.jsonc");
            if write_text(&path, &json_pretty(value)?, dry_run)? {
                result.permissions += 1;
            }
        }
    }
    if enabled(Feature::Ignore) {
        if let Some(value) = &model.ignore {
            let path = output_dir.join(".aiignore");
            if write_text(&path, value, dry_run)? {
                result.ignore += 1;
            }
        }
    }
    if enabled(Feature::Checks) {
        for check in &model.checks {
            let path = output_dir.join("checks").join(&check.relative_path);
            if write_text(
                &path,
                &stringify_frontmatter(&check.body, &check.frontmatter)?,
                dry_run,
            )? {
                result.checks += 1;
            }
        }
    }
    Ok(result)
}

pub fn import_tool_to_directory(
    target: &str,
    source_root: &Path,
    output_dir: &Path,
    features: &[String],
    dry_run: bool,
) -> Result<ImportResult> {
    let spec = target_spec(target).ok_or_else(|| anyhow!("Invalid tool target '{target}'"))?;
    let model = load_model_from_tool(&spec, source_root, false)?;
    write_canonical_model_at(&model, output_dir, features, dry_run, target != "kimi-code")
}

fn import_mcp_value(value: &Value, format: DataFormat, target: &str) -> Value {
    let mut servers = Map::new();
    let mut hermes_overrides = Map::new();
    match format {
        DataFormat::JsonServers => {
            if let Some(map) = value.get("servers").and_then(Value::as_object) {
                servers = map.clone();
            }
        }
        DataFormat::JsonAmp => {
            if let Some(map) = value.get("amp.mcpServers").and_then(Value::as_object) {
                servers = map.clone();
            }
        }
        DataFormat::JsonOpenCode => {
            if let Some(map) = value.get("mcp").and_then(Value::as_object) {
                for (name, raw) in map {
                    let mut server = raw.as_object().cloned().unwrap_or_default();
                    let kind = server
                        .remove("type")
                        .and_then(|v| v.as_str().map(ToOwned::to_owned));
                    if kind.as_deref() == Some("local") {
                        if let Some(Value::Array(command)) = server.remove("command") {
                            if let Some(first) = command.first().and_then(Value::as_str) {
                                server.insert("type".into(), Value::String("stdio".into()));
                                server.insert("command".into(), Value::String(first.into()));
                                if command.len() > 1 {
                                    server
                                        .insert("args".into(), Value::Array(command[1..].to_vec()));
                                }
                            }
                        }
                        if let Some(environment) = server.remove("environment") {
                            server.insert("env".into(), environment);
                        }
                    } else if kind.as_deref() == Some("remote") {
                        server.insert(
                            "type".into(),
                            Value::String(if target == "kilo" { "http" } else { "sse" }.into()),
                        );
                    }
                    if let Some(enabled) = server.get("enabled").and_then(Value::as_bool) {
                        if !enabled {
                            server.insert("disabled".into(), Value::Bool(true));
                        }
                        server.remove("enabled");
                    }
                    servers.insert(name.clone(), Value::Object(server));
                }
            }
            if let Some(tools) = value.get("tools").and_then(Value::as_object) {
                let names = servers.keys().cloned().collect::<Vec<_>>();
                for name in names {
                    let prefix = format!("{name}_");
                    let mut enabled_tools = Vec::new();
                    let mut disabled_tools = Vec::new();
                    for (tool_name, enabled) in tools {
                        if let Some(suffix) = tool_name.strip_prefix(&prefix) {
                            if enabled.as_bool() == Some(true) {
                                enabled_tools.push(Value::String(suffix.into()));
                            } else {
                                disabled_tools.push(Value::String(suffix.into()));
                            }
                        }
                    }
                    if let Some(Value::Object(server)) = servers.get_mut(&name) {
                        if !enabled_tools.is_empty() {
                            server.insert("enabledTools".into(), Value::Array(enabled_tools));
                        }
                        if !disabled_tools.is_empty() {
                            server.insert("disabledTools".into(), Value::Array(disabled_tools));
                        }
                    }
                }
            }
        }
        DataFormat::TomlCodex => {
            if let Some(map) = value.get("mcp_servers").and_then(Value::as_object) {
                for (name, raw) in map {
                    let mut server = raw.as_object().cloned().unwrap_or_default();
                    for (from, to) in [
                        ("enabled_tools", "enabledTools"),
                        ("disabled_tools", "disabledTools"),
                        ("env_vars", "envVars"),
                    ] {
                        if let Some(value) = server.remove(from) {
                            server.insert(to.into(), value);
                        }
                    }
                    servers.insert(name.clone(), Value::Object(server));
                }
            }
        }
        DataFormat::TomlGrok => {
            if let Some(map) = value.get("mcp_servers").and_then(Value::as_object) {
                for (name, raw) in map {
                    let Some(mut object) = raw.as_object().cloned() else {
                        continue;
                    };
                    if object.get("enabled").and_then(Value::as_bool) == Some(false) {
                        object.remove("enabled");
                        object.insert("disabled".into(), Value::Bool(true));
                    } else {
                        object.remove("enabled");
                    }
                    servers.insert(name.clone(), Value::Object(object));
                }
            }
        }
        DataFormat::TomlVibe => {
            if let Some(items) = value.get("mcp_servers").and_then(Value::as_array) {
                for item in items {
                    let Some(mut object) = item.as_object().cloned() else {
                        continue;
                    };
                    let Some(name) = object
                        .remove("name")
                        .and_then(|value| value.as_str().map(ToOwned::to_owned))
                    else {
                        continue;
                    };
                    if let Some(disabled_tools) = object.remove("disabled_tools") {
                        object.insert("disabledTools".into(), disabled_tools);
                    }
                    if let Some(transport) = object.get("transport").and_then(Value::as_str) {
                        object.insert("type".into(), Value::String(transport.into()));
                    }
                    servers.insert(name, Value::Object(object));
                }
            } else if let Some(map) = value.get("mcp_servers").and_then(Value::as_object) {
                servers = map.clone();
            }
        }
        DataFormat::TomlReasonix => {
            if let Some(items) = value.get("plugins").and_then(Value::as_array) {
                for item in items {
                    if let Some(mut object) = item.as_object().cloned() {
                        if let Some(name) = object.remove("name") {
                            if let Some(name) = name.as_str() {
                                servers.insert(name.into(), Value::Object(object));
                            }
                        }
                    }
                }
            }
        }
        DataFormat::YamlGoose => {
            if let Some(map) = value.get("extensions").and_then(Value::as_object) {
                for (name, raw) in map {
                    let Some(object) = raw.as_object() else {
                        continue;
                    };
                    let kind = object
                        .get("type")
                        .and_then(Value::as_str)
                        .or_else(|| object.get("cmd").map(|_| "stdio"))
                        .or_else(|| object.get("uri").map(|_| "streamable_http"));
                    let Some(kind) = kind else {
                        continue;
                    };
                    if !matches!(kind, "stdio" | "sse" | "streamable_http") {
                        continue;
                    }
                    let mut server = Map::new();
                    server.insert(
                        "type".into(),
                        Value::String(
                            if kind == "streamable_http" {
                                "http"
                            } else {
                                kind
                            }
                            .into(),
                        ),
                    );
                    if let Some(command) = object.get("cmd") {
                        server.insert("command".into(), command.clone());
                    }
                    if let Some(args) = object.get("args") {
                        server.insert("args".into(), args.clone());
                    }
                    if let Some(env) = object.get("envs") {
                        server.insert("env".into(), env.clone());
                    }
                    if let Some(uri) = object.get("uri") {
                        server.insert("url".into(), uri.clone());
                    }
                    if let Some(headers) = object.get("headers") {
                        server.insert("headers".into(), headers.clone());
                    }
                    if object.get("enabled").and_then(Value::as_bool) == Some(false) {
                        server.insert("disabled".into(), Value::Bool(true));
                    }
                    if let Some(timeout) = object.get("timeout") {
                        server.insert("networkTimeout".into(), timeout.clone());
                    }
                    servers.insert(name.clone(), Value::Object(server));
                }
            }
        }
        DataFormat::YamlHermes => {
            if let Some(map) = value.get("mcp_servers").and_then(Value::as_object) {
                for (name, raw) in map {
                    let Some(object) = raw.as_object() else {
                        continue;
                    };
                    let mut server = Map::new();
                    for key in ["command", "args", "env", "url", "headers"] {
                        if let Some(value) = object.get(key) {
                            server.insert(key.into(), value.clone());
                        }
                    }
                    if object.get("transport").and_then(Value::as_str) == Some("sse") {
                        server.insert("type".into(), Value::String("sse".into()));
                    }
                    if object.get("enabled").and_then(Value::as_bool) == Some(false) {
                        server.insert("disabled".into(), Value::Bool(true));
                    }
                    if let Some(timeout) = object.get("timeout") {
                        server.insert("networkTimeout".into(), timeout.clone());
                    }
                    if let Some(Value::Object(tools)) = object.get("tools") {
                        if let Some(value) = tools.get("include") {
                            server.insert("enabledTools".into(), value.clone());
                        }
                        if let Some(value) = tools.get("exclude") {
                            server.insert("disabledTools".into(), value.clone());
                        }
                    }
                    let mut advanced = Map::new();
                    for key in [
                        "auth",
                        "client_cert",
                        "client_key",
                        "connect_timeout",
                        "supports_parallel_tool_calls",
                        "oauth",
                        "idle_timeout_seconds",
                        "max_lifetime_seconds",
                        "ssl_verify",
                        "skip_preflight",
                        "sampling",
                        "keepalive_interval",
                        "elicitation",
                        "trust",
                        "identity_header",
                    ] {
                        if let Some(value) = object.get(key) {
                            advanced.insert(key.into(), value.clone());
                        }
                    }
                    if !advanced.is_empty() {
                        let mut override_server = server.clone();
                        override_server.extend(advanced);
                        hermes_overrides.insert(name.clone(), Value::Object(override_server));
                    }
                    servers.insert(name.clone(), Value::Object(server));
                }
            }
        }
        DataFormat::YamlTakt => {}
        DataFormat::JsonMcpServers | DataFormat::JsonGeneric => {
            let key = if target == "zed" {
                "context_servers"
            } else {
                "mcpServers"
            };
            if let Some(map) = value.get(key).and_then(Value::as_object) {
                servers = map.clone();
            }
        }
    }
    if target == "rovodev" {
        for value in servers.values_mut() {
            if let Some(object) = value.as_object_mut() {
                let transport = object.remove("transport");
                object.remove("disabled");
                if let Some(transport) =
                    transport.and_then(|value| value.as_str().map(str::to_owned))
                {
                    if matches!(transport.as_str(), "stdio" | "http" | "sse") {
                        object.insert("type".into(), Value::String(transport));
                    }
                }
            }
        }
    }
    if target == "kimi-code" {
        for value in servers.values_mut() {
            if let Some(object) = value.as_object_mut() {
                if let Some(transport) = object.remove("transport") {
                    if matches!(transport.as_str(), Some("stdio" | "sse" | "http")) {
                        object.insert("type".into(), transport);
                    }
                }
                if object.get("enabled").and_then(Value::as_bool) == Some(false) {
                    object.insert("disabled".into(), Value::Bool(true));
                    object.remove("enabled");
                }
            }
        }
    }
    if target == "deepagents" {
        for value in servers.values_mut() {
            if let Some(object) = value.as_object_mut() {
                if let Some(allowed) = object.remove("allowedTools") {
                    object.insert("enabledTools".into(), allowed);
                }
                for key in ["type", "transport"] {
                    if let Some(Value::String(value)) = object.get_mut(key) {
                        if matches!(value.as_str(), "streamable_http" | "streamable-http") {
                            *value = "http".into();
                        }
                    }
                }
            }
        }
    }
    if target == "copilotcli" {
        for value in servers.values_mut() {
            if let Some(object) = value.as_object_mut() {
                if let Some(tools) = object.remove("tools") {
                    if tools.is_array() {
                        object.insert("enabledTools".into(), tools);
                    } else {
                        object.insert("tools".into(), tools);
                    }
                }
                if object.get("type").and_then(Value::as_str) == Some("stdio") {
                    object.remove("type");
                }
            }
        }
    }
    if target == "warp" {
        for value in servers.values_mut() {
            if let Some(object) = value.as_object_mut() {
                if let Some(working_directory) = object.remove("working_directory") {
                    object.insert("cwd".into(), working_directory);
                }
            }
        }
    }
    if target.starts_with("antigravity-") {
        for value in servers.values_mut() {
            if let Some(object) = value.as_object_mut() {
                if let Some(url) = object.remove("serverUrl") {
                    object.insert("url".into(), url);
                }
            }
        }
    }
    let mut result = Map::from_iter([("mcpServers".into(), Value::Object(servers))]);
    if target == "hermesagent" && !hermes_overrides.is_empty() {
        result.insert(
            "hermesagent".into(),
            Value::Object(Map::from_iter([(
                "mcpServers".into(),
                Value::Object(hermes_overrides),
            )])),
        );
    }
    Value::Object(result)
}

fn canonical_hook_event_name(event: &str, target: &str) -> String {
    let mapped = match target {
        "opencode" | "kilo" => match event {
            "session.created" => "sessionStart",
            "tool.execute.before" => "preToolUse",
            "tool.execute.after" => "postToolUse",
            "session.idle" => "stop",
            "file.edited" => "afterFileEdit",
            "permission.asked" => "permissionRequest",
            "permission.replied" => "permissionDenied",
            "tui.toast.show" => "notification",
            "experimental.session.compacting" => "preCompact",
            "chat.message" => "beforeSubmitPrompt",
            "session.compacted" => "postCompact",
            "session.error" => "afterError",
            "file.watcher.updated" => "fileChanged",
            _ => event,
        },
        "pi" => match event {
            "session_start" => "sessionStart",
            "session_shutdown" => "sessionEnd",
            "tool_call" => "preToolUse",
            "tool_result" => "postToolUse",
            "context" => "preModelInvocation",
            "message_end" => "postModelInvocation",
            "input" => "beforeSubmitPrompt",
            "agent_end" => "stop",
            "session_before_compact" => "preCompact",
            "session_compact" => "postCompact",
            _ => event,
        },
        "amp" => match event {
            "session.start" => "sessionStart",
            "tool.call" => "preToolUse",
            "tool.result" => "postToolUse",
            "agent.start" => "beforeSubmitPrompt",
            "agent.end" => "stop",
            _ => event,
        },
        "cline" => match event {
            "TaskStart" => "sessionStart",
            "SessionShutdown" => "sessionEnd",
            "PreToolUse" => "preToolUse",
            "PostToolUse" => "postToolUse",
            "UserPromptSubmit" => "beforeSubmitPrompt",
            "PreCompact" => "preCompact",
            "Notification" => "notification",
            "TaskComplete" => "taskCompleted",
            "TaskError" => "afterError",
            _ => event,
        },
        "copilot" => match event {
            "userPromptSubmitted" => "beforeSubmitPrompt",
            "agentStop" => "stop",
            "errorOccurred" => "afterError",
            "userPromptTransformed" => "userPromptExpansion",
            _ => event,
        },
        "copilotcli" => match event {
            "userPromptSubmitted" => "beforeSubmitPrompt",
            "agentStop" => "stop",
            "errorOccurred" => "afterError",
            "userPromptTransformed" => "userPromptExpansion",
            "preMcpToolCall" => "beforeMCPExecution",
            _ => event,
        },
        "kiro" => match event {
            "agentSpawn" => "sessionStart",
            "stop" => "stop",
            "userPromptSubmit" => "beforeSubmitPrompt",
            "preToolUse" => "preToolUse",
            "postToolUse" => "postToolUse",
            _ => event,
        },
        "kiro-cli" | "kiro-ide" => match event {
            "SessionStart" => "sessionStart",
            "UserPromptSubmit" => "beforeSubmitPrompt",
            "PreToolUse" => "preToolUse",
            "PostToolUse" => "postToolUse",
            "Stop" => "stop",
            _ => event,
        },
        "antigravity-cli" | "antigravity-ide" | "antigravity-plugin" => match event {
            "PreToolUse" => "preToolUse",
            "PostToolUse" => "postToolUse",
            "PreInvocation" => "preModelInvocation",
            "PostInvocation" => "postModelInvocation",
            "Stop" => "stop",
            _ => event,
        },
        "hermesagent" => match event {
            "on_session_start" => "sessionStart",
            "on_session_end" => "sessionEnd",
            "pre_tool_call" => "preToolUse",
            "post_tool_call" => "postToolUse",
            "on_user_prompt" => "beforeSubmitPrompt",
            "on_agent_end" => "stop",
            _ => event,
        },
        _ => event,
    };
    if mapped != event {
        mapped.to_owned()
    } else if matches!(
        target,
        "claudecode"
            | "claudecode-legacy"
            | "codexcli"
            | "qwencode"
            | "augmentcode"
            | "reasonix"
            | "grokcli"
            | "factorydroid"
            | "junie"
            | "goose"
            | "devin"
            | "deepagents"
    ) {
        pascal_to_camel(event)
    } else {
        mapped.to_owned()
    }
}

fn strip_kimi_hook_wrapper(command: &str) -> String {
    let posix_prefix = "export CARABINER_KIMI_HOOK_CWD=1 && cd -- '";
    if let Some(rest) = command.strip_prefix(posix_prefix) {
        if let Some(index) = rest.find("' && ") {
            return rest[index + 5..].to_owned();
        }
    }
    let windows_prefix = "set \"CARABINER_KIMI_HOOK_CWD=1\" && cd /d \"";
    if let Some(rest) = command.strip_prefix(windows_prefix) {
        if let Some(index) = rest.find("\" && ") {
            return rest[index + 5..].to_owned();
        }
    }
    command.to_owned()
}

fn import_kimi_hooks_value(value: &Value) -> Value {
    let mut hooks = Map::new();
    if let Some(entries) = value.get("hooks").and_then(Value::as_array) {
        for entry in entries {
            let Some(entry) = entry.as_object() else {
                continue;
            };
            let Some(native_event) = entry.get("event").and_then(Value::as_str) else {
                continue;
            };
            let canonical_event = match native_event {
                "SessionStart" => "sessionStart",
                "SessionEnd" => "sessionEnd",
                "PreToolUse" => "preToolUse",
                "PostToolUse" => "postToolUse",
                "UserPromptSubmit" => "beforeSubmitPrompt",
                "Stop" => "stop",
                "PreCompact" => "preCompact",
                "PostCompact" => "postCompact",
                _ => native_event,
            };
            let Some(command) = entry.get("command").and_then(Value::as_str) else {
                continue;
            };
            let mut definition = Map::new();
            definition.insert("type".into(), Value::String("command".into()));
            definition.insert(
                "command".into(),
                Value::String(strip_kimi_hook_wrapper(command)),
            );
            for (from, to) in [("matcher", "matcher"), ("timeout", "timeout")] {
                if let Some(value) = entry.get(from) {
                    definition.insert(to.into(), value.clone());
                }
            }
            hooks
                .entry(canonical_event)
                .or_insert_with(|| Value::Array(Vec::new()))
                .as_array_mut()
                .expect("hook entry is initialized as an array")
                .push(Value::Object(definition));
        }
    }
    Value::Object(Map::from_iter([("hooks".into(), Value::Object(hooks))]))
}

fn import_hermes_hooks_value(value: &Value) -> Value {
    let mut canonical = Map::new();
    let mut native_only = Map::new();
    let Some(root) = value.as_object() else {
        return Value::Object(Map::from_iter([("hooks".into(), Value::Object(canonical))]));
    };
    if let Some(hooks) = root.get("hooks").and_then(Value::as_object) {
        for (native_event, definitions) in hooks {
            let Some(definitions) = definitions.as_array() else {
                continue;
            };
            let mut converted = Vec::new();
            for definition in definitions {
                let Some(definition) = definition.as_object() else {
                    continue;
                };
                let Some(command) = definition.get("command").and_then(Value::as_str) else {
                    continue;
                };
                let mut converted_definition = Map::new();
                converted_definition.insert("type".into(), Value::String("command".into()));
                converted_definition.insert("command".into(), Value::String(command.into()));
                for key in ["matcher", "timeout"] {
                    if let Some(value) = definition.get(key) {
                        converted_definition.insert(key.into(), value.clone());
                    }
                }
                if let Some(value) = definition.get("fail_closed") {
                    converted_definition.insert("failClosed".into(), value.clone());
                }
                converted.push(Value::Object(converted_definition));
            }
            if converted.is_empty() {
                continue;
            }
            if let Some(event) = hermes_canonical_event(native_event) {
                canonical.insert(event.into(), Value::Array(converted));
            } else if hermes_native_event(native_event).is_some() {
                native_only.insert(native_event.clone(), Value::Array(converted));
            }
        }
    }
    let mut result = Map::new();
    result.insert("hooks".into(), Value::Object(canonical));
    if !native_only.is_empty() {
        result.insert(
            "hermesagent".into(),
            Value::Object(Map::from_iter([(
                "hooks".into(),
                Value::Object(native_only),
            )])),
        );
    }
    Value::Object(result)
}

fn import_vibe_hooks_value(value: &Value) -> Value {
    let mut hooks = Map::new();
    if let Some(entries) = value.get("hooks").and_then(Value::as_array) {
        for entry in entries {
            let Some(entry) = entry.as_object() else {
                continue;
            };
            let Some(event) = entry.get("type").and_then(Value::as_str) else {
                continue;
            };
            let canonical_event = match event {
                "pre_tool" | "before_tool" => "preToolUse",
                "post_tool" | "after_tool" => "postToolUse",
                "post_agent" | "post_agent_turn" => "stop",
                _ => continue,
            };
            let mut definition = Map::new();
            definition.insert("type".into(), Value::String("command".into()));
            for (from, to) in [
                ("command", "command"),
                ("match", "matcher"),
                ("timeout", "timeout"),
                ("name", "name"),
                ("description", "description"),
                ("strict", "strict"),
            ] {
                if let Some(value) = entry.get(from) {
                    definition.insert(to.into(), value.clone());
                }
            }
            hooks
                .entry(canonical_event)
                .or_insert_with(|| Value::Array(Vec::new()))
                .as_array_mut()
                .expect("hook entry is initialized as an array")
                .push(Value::Object(definition));
        }
    }
    Value::Object(Map::from_iter([
        ("version".into(), Value::Number(1.into())),
        ("hooks".into(), Value::Object(hooks)),
    ]))
}

fn normalize_import_hook_definitions(value: &Value) -> Value {
    let Some(values) = value.as_array() else {
        return value.clone();
    };
    let mut result = Vec::new();
    for raw in values {
        let Some(object) = raw.as_object() else {
            continue;
        };
        if let Some(inner) = object.get("hooks").and_then(Value::as_array) {
            for raw_inner in inner {
                let Some(inner) = raw_inner.as_object() else {
                    continue;
                };
                let mut definition = inner.clone();
                if !definition.contains_key("matcher") {
                    if let Some(matcher) = object.get("matcher") {
                        definition.insert("matcher".into(), matcher.clone());
                    }
                }
                if definition.get("type").is_none() && definition.get("command").is_some() {
                    definition.insert("type".into(), Value::String("command".into()));
                }
                result.push(Value::Object(definition));
            }
        } else {
            let mut definition = object.clone();
            if definition.get("type").is_none() && definition.get("command").is_some() {
                definition.insert("type".into(), Value::String("command".into()));
            }
            result.push(Value::Object(definition));
        }
    }
    Value::Array(result)
}
fn strip_cursor_command_types(value: Value) -> Value {
    let Value::Array(values) = value else {
        return value;
    };
    Value::Array(
        values
            .into_iter()
            .map(|value| {
                let Value::Object(mut object) = value else {
                    return value;
                };
                if object.get("type").and_then(Value::as_str) == Some("command") {
                    object.remove("type");
                }
                Value::Object(object)
            })
            .collect(),
    )
}

fn import_reasonix_hooks_value(value: &Value) -> Value {
    let mut hooks = Map::new();
    let source = value
        .get("hooks")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    for (event, raw_entries) in source {
        let Some(entries) = raw_entries.as_array() else {
            continue;
        };
        let mut definitions = Vec::new();
        for raw_entry in entries {
            let Some(entry) = raw_entry.as_object() else {
                continue;
            };
            let Some(command) = entry.get("command").and_then(Value::as_str) else {
                continue;
            };
            if command.is_empty() {
                continue;
            }
            let mut definition = Map::new();
            definition.insert("type".into(), Value::String("command".into()));
            definition.insert("command".into(), Value::String(command.into()));
            if let Some(matcher) = entry.get("match").filter(|value| value.is_string()) {
                if matcher.as_str().is_some_and(|value| !value.is_empty()) {
                    definition.insert("matcher".into(), matcher.clone());
                }
            }
            if let Some(description) = entry.get("description").filter(|value| value.is_string()) {
                if description.as_str().is_some_and(|value| !value.is_empty()) {
                    definition.insert("description".into(), description.clone());
                }
            }
            if let Some(timeout) = entry.get("timeout").and_then(Value::as_f64) {
                let seconds = timeout / 1000.0;
                let value = if seconds.fract() == 0.0 {
                    Value::Number(serde_json::Number::from(seconds as i64))
                } else {
                    Value::Number(
                        serde_json::Number::from_f64(seconds)
                            .expect("finite Reasonix hook timeout"),
                    )
                };
                definition.insert("timeout".into(), value);
            }
            definitions.push(Value::Object(definition));
        }
        if !definitions.is_empty() {
            hooks.insert(
                canonical_hook_event_name(&event, "reasonix"),
                Value::Array(definitions),
            );
        }
    }
    Value::Object(Map::from_iter([
        ("version".into(), Value::Number(1.into())),
        ("hooks".into(), Value::Object(hooks)),
    ]))
}

fn import_devin_hooks_value(value: &Value) -> Value {
    let Some(root) = value.as_object() else {
        return Value::Object(Map::from_iter([(
            "hooks".into(),
            Value::Object(Map::new()),
        )]));
    };
    if root.contains_key("hooks") {
        return import_hooks_value(value, "devin");
    }
    let mut hooks = Map::new();
    for (event, definitions) in root {
        let canonical = canonical_hook_event_name(event, "devin");
        let normalized = normalize_import_hook_definitions(definitions);
        if normalized
            .as_array()
            .is_some_and(|values| !values.is_empty())
        {
            hooks.insert(canonical, normalized);
        }
    }
    Value::Object(Map::from_iter([
        ("version".into(), Value::Number(1.into())),
        ("hooks".into(), Value::Object(hooks)),
    ]))
}

fn import_hooks_value(value: &Value, target: &str) -> Value {
    let mut result = match value {
        Value::Object(map) => map.clone(),
        _ => Map::new(),
    };
    if target == "kiro" {
        let source_hooks = result
            .get("hooks")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_else(|| result.clone());
        let mut hooks = Map::new();
        for (event, value) in source_hooks {
            let Some(definitions) = value.as_array() else {
                continue;
            };
            let mut canonical_definitions = Vec::new();
            for definition in definitions {
                let Some(object) = definition.as_object() else {
                    continue;
                };
                let Some(command) = object.get("command").and_then(Value::as_str) else {
                    continue;
                };
                if command.is_empty() {
                    continue;
                }
                let mut canonical = Map::new();
                canonical.insert("type".into(), Value::String("command".into()));
                canonical.insert("command".into(), Value::String(command.into()));
                for key in ["matcher", "name", "description"] {
                    if let Some(value) = object.get(key).filter(|value| value.is_string()) {
                        canonical.insert(key.into(), value.clone());
                    }
                }
                if let Some(timeout) = object.get("timeout_ms").filter(|value| value.is_number()) {
                    canonical.insert("timeout".into(), timeout.clone());
                }
                if let Some(cache_ttl) = object
                    .get("cache_ttl_seconds")
                    .filter(|value| value.is_number())
                {
                    canonical.insert("cacheTtl".into(), cache_ttl.clone());
                }
                canonical_definitions.push(Value::Object(canonical));
            }
            if !canonical_definitions.is_empty() {
                hooks.insert(
                    canonical_hook_event_name(&event, "kiro"),
                    Value::Array(canonical_definitions),
                );
            }
        }
        return Value::Object(Map::from_iter([
            ("version".into(), Value::Number(1.into())),
            ("hooks".into(), Value::Object(hooks)),
        ]));
    }
    if matches!(target, "kiro-cli" | "kiro-ide") {
        let entries = result
            .remove("hooks")
            .and_then(|value| value.as_array().cloned())
            .unwrap_or_default();
        let mut hooks = Map::new();
        for entry in entries {
            let Some(object) = entry.as_object() else {
                continue;
            };
            let Some(trigger) = object.get("trigger").and_then(Value::as_str) else {
                continue;
            };
            let Some(action) = object.get("action").and_then(Value::as_object) else {
                continue;
            };
            let Some(action_type) = action.get("type").and_then(Value::as_str) else {
                continue;
            };
            let mut definition = Map::new();
            match action_type {
                "command" => {
                    let Some(command) = action.get("command").and_then(Value::as_str) else {
                        continue;
                    };
                    if command.is_empty() {
                        continue;
                    }
                    definition.insert("type".into(), Value::String("command".into()));
                    definition.insert("command".into(), Value::String(command.into()));
                }
                "agent" => {
                    let Some(prompt) = action.get("prompt").and_then(Value::as_str) else {
                        continue;
                    };
                    if prompt.is_empty() {
                        continue;
                    }
                    definition.insert("type".into(), Value::String("prompt".into()));
                    definition.insert("prompt".into(), Value::String(prompt.into()));
                }
                _ => continue,
            }
            for key in ["name", "description", "matcher", "timeout"] {
                if let Some(value) = object.get(key) {
                    definition.insert(key.into(), value.clone());
                }
            }
            if object.get("enabled").and_then(Value::as_bool) == Some(false) {
                definition.insert("enabled".into(), Value::Bool(false));
            }
            let event = canonical_hook_event_name(trigger, target);
            hooks
                .entry(event)
                .or_insert_with(|| Value::Array(Vec::new()))
                .as_array_mut()
                .expect("hook bucket is an array")
                .push(Value::Object(definition));
        }
        return Value::Object(Map::from_iter([
            ("version".into(), Value::Number(1.into())),
            ("hooks".into(), Value::Object(hooks)),
        ]));
    }
    let hook_map = if matches!(
        target,
        "antigravity-cli" | "antigravity-ide" | "antigravity-plugin"
    ) {
        result
            .remove("carabiner")
            .and_then(|value| value.as_object().cloned())
    } else if target == "factorydroid" {
        Some(result.clone())
    } else {
        result
            .remove("hooks")
            .and_then(|value| value.as_object().cloned())
    };
    let hooks = hook_map
        .map(|hooks| {
            hooks
                .into_iter()
                .map(|(key, value)| {
                    let definitions = normalize_import_hook_definitions(&value);
                    let definitions = if target == "cursor" {
                        strip_cursor_command_types(definitions)
                    } else {
                        definitions
                    };
                    (canonical_hook_event_name(&key, target), definitions)
                })
                .collect()
        })
        .unwrap_or_default();
    let version = result
        .remove("version")
        .unwrap_or_else(|| Value::Number(1.into()));
    Value::Object(Map::from_iter([
        ("version".into(), version),
        ("hooks".into(), Value::Object(hooks)),
    ]))
}

fn pascal_to_camel(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn parse_permission_entry(entry: &str, target: &str) -> Option<(String, String)> {
    let trimmed = entry.trim();
    let open = trimmed.find('(');
    let (tool, pattern): (&str, String) = match open {
        Some(index) if trimmed.ends_with(')') => (
            &trimmed[..index],
            trimmed[index + 1..trimmed.len() - 1].to_owned(),
        ),
        Some(index) => (&trimmed[..index], "*".into()),
        None => (trimmed, "*".into()),
    };
    let category = if target == "grokcli" && tool == "MCPTool" {
        if pattern == "*" {
            "mcp".into()
        } else {
            format!("mcp__{pattern}")
        }
    } else if target == "antigravity-cli" || target == "antigravity-ide" {
        match tool {
            "read_file" => "read".into(),
            "write_file" => "write".into(),
            "command" => "bash".into(),
            "read_url" => "webfetch".into(),
            "mcp" => "mcp".into(),
            other => other.to_ascii_lowercase(),
        }
    } else if target == "cursor" {
        match tool {
            "Shell" => "bash".into(),
            "Read" => "read".into(),
            "Edit" => "edit".into(),
            "Write" => "write".into(),
            "Fetch" => "webfetch".into(),
            other => other.to_ascii_lowercase(),
        }
    } else if target == "qwencode" {
        match tool {
            "Bash" => "bash".into(),
            "Read" => "read".into(),
            "Edit" => "edit".into(),
            "Write" => "write".into(),
            "WebFetch" => "webfetch".into(),
            "WebSearch" => "websearch".into(),
            "Grep" => "grep".into(),
            "Glob" => "glob".into(),
            "Agent" => "agent".into(),
            other => other.into(),
        }
    } else if target == "reasonix" {
        match tool {
            "Bash" => "bash".into(),
            "Read" => "read".into(),
            "Edit" => "edit".into(),
            "Write" => "write".into(),
            "WebFetch" => "webfetch".into(),
            "WebSearch" => "websearch".into(),
            "Grep" => "grep".into(),
            "Glob" => "glob".into(),
            "NotebookEdit" => "notebookedit".into(),
            "Agent" => "agent".into(),
            other => other.into(),
        }
    } else {
        match tool {
            "Bash" => "bash".into(),
            "Read" => "read".into(),
            "Edit" => "edit".into(),
            "Write" => "write".into(),
            "WebFetch" => "webfetch".into(),
            "WebSearch" => "websearch".into(),
            "Grep" => "grep".into(),
            "Glob" => "glob".into(),
            "Agent" => "agent".into(),
            "MCPTool" => "mcp".into(),
            other => other.to_ascii_lowercase(),
        }
    };
    Some((
        category,
        if pattern.is_empty() {
            "*".into()
        } else {
            pattern
        },
    ))
}

fn import_permission_arrays(value: &Value, key: &str, target: &str) -> Map<String, Value> {
    let mut categories = Map::new();
    let Some(permissions) = value.get(key).and_then(Value::as_object) else {
        return categories;
    };
    for (action_key, action) in [("allow", "allow"), ("ask", "ask"), ("deny", "deny")] {
        if let Some(Value::Array(entries)) = permissions.get(action_key) {
            for entry in entries.iter().filter_map(Value::as_str) {
                if let Some((category, pattern)) = parse_permission_entry(entry, target) {
                    let bucket = categories
                        .entry(category)
                        .or_insert_with(|| Value::Object(Map::new()));
                    if let Some(map) = bucket.as_object_mut() {
                        map.insert(pattern, Value::String(action.into()));
                    }
                }
            }
        }
    }
    categories
}

fn augment_shell_regex_to_glob(regex: &str) -> String {
    let mut value = regex;
    if let Some(stripped) = value.strip_prefix('^') {
        value = stripped;
    }
    if let Some(stripped) = value.strip_suffix('$') {
        value = stripped;
    }
    let mut glob = String::new();
    let mut chars = value.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '\\' {
            if let Some(next) = chars.next() {
                glob.push(next);
            }
        } else if character == '.' && chars.peek() == Some(&'*') {
            chars.next();
            glob.push('*');
        } else if character == '.' {
            glob.push('?');
        } else {
            glob.push(character);
        }
    }
    glob
}

fn import_augment_permissions_value(value: &Value) -> Value {
    let mut categories = Map::new();
    let mut special = Vec::new();
    if let Some(entries) = value.get("toolPermissions").and_then(Value::as_array) {
        for entry in entries {
            let Some(object) = entry.as_object() else {
                continue;
            };
            let Some(tool_name) = object.get("toolName").and_then(Value::as_str) else {
                continue;
            };
            let Some(permission) = object.get("permission").and_then(Value::as_object) else {
                continue;
            };
            let kind = permission.get("type").and_then(Value::as_str).unwrap_or("");
            let is_special = !matches!(kind, "allow" | "deny" | "ask-user")
                || object
                    .get("eventType")
                    .and_then(Value::as_str)
                    .is_some_and(|event| event != "tool-call")
                || permission.get("webhookUrl").is_some()
                || permission.get("script").is_some();
            if is_special {
                special.push(entry.clone());
                continue;
            }
            let category = match tool_name {
                "launch-process" => "bash",
                "view" => "read",
                "str-replace-editor" => "edit",
                "save-file" => "write",
                "web-fetch" => "webfetch",
                "web-search" => "websearch",
                other => other,
            };
            let action = match kind {
                "allow" => "allow",
                "deny" => "deny",
                "ask-user" => "ask",
                _ => unreachable!(),
            };
            let pattern = object
                .get("shellInputRegex")
                .and_then(Value::as_str)
                .map(augment_shell_regex_to_glob)
                .filter(|pattern| !pattern.is_empty())
                .unwrap_or_else(|| "*".into());
            categories
                .entry(category)
                .or_insert_with(|| Value::Object(Map::new()))
                .as_object_mut()
                .expect("permission category is an object")
                .insert(pattern, Value::String(action.into()));
        }
    }
    let mut result = Map::from_iter([("permission".into(), Value::Object(categories))]);
    if !special.is_empty() {
        result.insert(
            "augmentcode".into(),
            Value::Object(Map::from_iter([(
                "toolPermissions".into(),
                Value::Array(special),
            )])),
        );
    }
    Value::Object(result)
}

fn import_kilo_permissions_value(value: &Value) -> Value {
    let raw_permission = value
        .get("permission")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let shared_names = [
        "bash",
        "read",
        "edit",
        "write",
        "webfetch",
        "websearch",
        "grep",
        "glob",
        "notebookedit",
        "agent",
    ];
    let mut shared = Map::new();
    let mut override_only = Map::new();
    for (key, value) in raw_permission {
        if shared_names.iter().any(|name| *name == key) || key.starts_with("mcp__") || key == "*" {
            let value = if value.is_string() {
                Value::Object(Map::from_iter([("*".into(), value)]))
            } else {
                value
            };
            shared.insert(key, value);
        } else {
            override_only.insert(key, value);
        }
    }
    let mut kilo = Map::new();
    if !override_only.is_empty() {
        kilo.insert("permission".into(), Value::Object(override_only));
    }
    if let Some(Value::Object(sandbox)) = value.get("sandbox") {
        kilo.insert("sandbox".into(), Value::Object(sandbox.clone()));
    }
    let mut result = Map::from_iter([("permission".into(), Value::Object(shared))]);
    if !kilo.is_empty() {
        result.insert("kilo".into(), Value::Object(kilo));
    }
    Value::Object(result)
}

fn add_codex_permission_rule(
    permissions: &mut Map<String, Value>,
    category: &str,
    pattern: &str,
    action: &str,
) {
    permissions
        .entry(category)
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .expect("Codex permission category is an object")
        .insert(pattern.into(), Value::String(action.into()));
}

fn import_permissions_value(value: &Value, target: &str) -> Value {
    if target == "augmentcode" {
        return import_augment_permissions_value(value);
    }
    if target == "kilo" {
        return import_kilo_permissions_value(value);
    }
    if target == "codexcli" {
        let mut permission = Map::new();
        let default_profile = value
            .get("default_permissions")
            .and_then(Value::as_str)
            .unwrap_or("carabiner");
        let profile = value
            .get("permissions")
            .and_then(|permissions| permissions.get(default_profile))
            .and_then(Value::as_object);
        if let Some(profile) = profile {
            if let Some(Value::Object(filesystem)) = profile.get("filesystem") {
                for (pattern, access) in filesystem {
                    if pattern == ":minimal" {
                        continue;
                    }
                    let Some(access) = access.as_str() else {
                        continue;
                    };
                    match access {
                        "deny" | "none" => {
                            add_codex_permission_rule(&mut permission, "read", pattern, "deny");
                            add_codex_permission_rule(&mut permission, "edit", pattern, "deny");
                        }
                        "write" => {
                            add_codex_permission_rule(&mut permission, "edit", pattern, "allow");
                        }
                        "read" => {
                            add_codex_permission_rule(&mut permission, "read", pattern, "allow");
                        }
                        _ => {}
                    }
                }
                if let Some(Value::Object(roots)) = filesystem.get(":workspace_roots") {
                    for (pattern, access) in roots {
                        if pattern == ".git/**" && access.as_str() == Some("write") {
                            continue;
                        }
                        let Some(access) = access.as_str() else {
                            continue;
                        };
                        match access {
                            "deny" | "none" => {
                                add_codex_permission_rule(&mut permission, "read", pattern, "deny");
                                add_codex_permission_rule(&mut permission, "edit", pattern, "deny");
                            }
                            "write" => {
                                add_codex_permission_rule(
                                    &mut permission,
                                    "edit",
                                    pattern,
                                    "allow",
                                );
                            }
                            "read" => {
                                add_codex_permission_rule(
                                    &mut permission,
                                    "read",
                                    pattern,
                                    "allow",
                                );
                            }
                            _ => {}
                        }
                    }
                }
            }
            if let Some(Value::Object(network)) = profile.get("network") {
                if let Some(Value::Object(domains)) = network.get("domains") {
                    let mut rules = Map::new();
                    for (domain, action) in domains {
                        if let Some(action) = action.as_str() {
                            rules.insert(domain.clone(), Value::String(action.into()));
                        }
                    }
                    if !rules.is_empty() {
                        permission.insert("webfetch".into(), Value::Object(rules));
                    }
                }
            }
        }
        let mut result = Map::from_iter([("permission".into(), Value::Object(permission))]);
        let mut override_fields = Map::new();
        if let Some(extends) = profile
            .and_then(|profile| profile.get("extends"))
            .and_then(Value::as_str)
        {
            override_fields.insert(
                "base_permission_profile".into(),
                Value::String(extends.into()),
            );
        }
        for key in [
            "approval_policy",
            "sandbox_mode",
            "sandbox_workspace_write",
            "apps",
            "approvals_reviewer",
            "tui",
        ] {
            if let Some(value) = value.get(key) {
                override_fields.insert(key.into(), value.clone());
            }
        }
        if !override_fields.is_empty() {
            result.insert("codexcli".into(), Value::Object(override_fields));
        }
        return Value::Object(result);
    }
    if matches!(
        target,
        "antigravity-cli"
            | "antigravity-ide"
            | "qwencode"
            | "reasonix"
            | "claudecode"
            | "claudecode-legacy"
            | "cursor"
    ) {
        let categories = import_permission_arrays(value, "permissions", target);
        return Value::Object(Map::from_iter([(
            "permission".into(),
            Value::Object(categories),
        )]));
    }
    if target == "grokcli" {
        let categories = import_permission_arrays(value, "permission", target);
        return Value::Object(Map::from_iter([(
            "permission".into(),
            Value::Object(categories),
        )]));
    }
    if target == "amp" {
        let mut categories = Map::new();
        if let Some(Value::Array(disabled)) = value.get("amp.tools.disable") {
            for tool in disabled.iter().filter_map(Value::as_str) {
                categories.insert(
                    tool.into(),
                    Value::Object(Map::from_iter([("*".into(), Value::String("deny".into()))])),
                );
            }
        }
        if let Some(Value::Array(entries)) = value.get("amp.permissions") {
            for entry in entries {
                if let Some(object) = entry.as_object() {
                    let tool = object.get("tool").and_then(Value::as_str).unwrap_or("");
                    let pattern = object
                        .get("matches")
                        .and_then(|matches| matches.get("cmd"))
                        .and_then(Value::as_str)
                        .unwrap_or("*");
                    let action = match object.get("action").and_then(Value::as_str) {
                        Some("reject") => "deny",
                        Some("allow") => "allow",
                        Some("ask") => "ask",
                        _ => continue,
                    };
                    let bucket = categories
                        .entry(tool)
                        .or_insert_with(|| Value::Object(Map::new()));
                    if let Some(map) = bucket.as_object_mut() {
                        map.insert(pattern.into(), Value::String(action.into()));
                    }
                }
            }
        }
        return Value::Object(Map::from_iter([(
            "permission".into(),
            Value::Object(categories),
        )]));
    }
    if target == "cline" {
        let mut bash = Map::new();
        for (key, action) in [("allow", "allow"), ("deny", "deny")] {
            if let Some(Value::Array(entries)) = value.get(key) {
                for entry in entries.iter().filter_map(Value::as_str) {
                    bash.insert(entry.into(), Value::String(action.into()));
                }
            }
        }
        return Value::Object(Map::from_iter([(
            "permission".into(),
            Value::Object(Map::from_iter([("bash".into(), Value::Object(bash))])),
        )]));
    }
    if target == "factorydroid" {
        let mut bash = Map::new();
        if let Some(Value::Array(entries)) = value.get("commandAllowlist") {
            for entry in entries.iter().filter_map(Value::as_str) {
                bash.insert(entry.into(), Value::String("allow".into()));
            }
        }
        if let Some(Value::Array(entries)) = value.get("commandDenylist") {
            for entry in entries.iter().filter_map(Value::as_str) {
                bash.insert(entry.into(), Value::String("deny".into()));
            }
        }
        let mut result = Map::from_iter([(
            "permission".into(),
            Value::Object(Map::from_iter([("bash".into(), Value::Object(bash))])),
        )]);
        let mut override_fields = Map::new();
        for key in [
            "commandBlocklist",
            "networkPolicy",
            "sandbox",
            "mcpPolicy",
            "mcpAutonomyOverrides",
            "enableDroidShield",
            "sessionDefaultSettings",
            "maxAutonomyLevel",
            "subagentAutonomyLevel",
            "interactionMode",
            "extraKnownMarketplaces",
            "enabledPlugins",
            "hooksDisabled",
            "disabledSkills",
        ] {
            if let Some(value) = value.get(key) {
                override_fields.insert(key.into(), value.clone());
            }
        }
        if !override_fields.is_empty() {
            result.insert("factorydroid".into(), Value::Object(override_fields));
        }
        return Value::Object(result);
    }
    if target == "devin" {
        let mut categories = Map::new();
        if let Some(Value::Object(permissions)) = value.get("permissions") {
            for (action_key, action) in [("allow", "allow"), ("ask", "ask"), ("deny", "deny")] {
                if let Some(Value::Array(entries)) = permissions.get(action_key) {
                    for entry in entries.iter().filter_map(Value::as_str) {
                        let (scope, pattern) = entry
                            .split_once('(')
                            .map(|(scope, rest)| (scope, rest.trim_end_matches(')')))
                            .unwrap_or((entry, "*"));
                        let category = match scope {
                            "Read" => "read",
                            "Write" => "write",
                            "Exec" => "bash",
                            "Fetch" => "webfetch",
                            other => other,
                        };
                        let bucket = categories
                            .entry(category)
                            .or_insert_with(|| Value::Object(Map::new()));
                        if let Some(map) = bucket.as_object_mut() {
                            map.insert(pattern.into(), Value::String(action.into()));
                        }
                    }
                }
            }
        }
        return Value::Object(Map::from_iter([(
            "permission".into(),
            Value::Object(categories),
        )]));
    }
    if target == "zed" {
        let mut categories = Map::new();
        if let Some(tools) = value
            .get("agent")
            .and_then(|agent| agent.get("tool_permissions"))
            .and_then(|permissions| permissions.get("tools"))
            .and_then(Value::as_object)
        {
            for (native_tool, raw_tool) in tools {
                let category = match native_tool.as_str() {
                    "terminal" => "bash",
                    "edit_file" => "edit",
                    "write_file" => "write",
                    "fetch" => "webfetch",
                    "search_web" => "websearch",
                    "read_file" => "read",
                    _ => continue,
                };
                let Some(tool) = raw_tool.as_object() else {
                    continue;
                };
                let bucket = categories
                    .entry(category)
                    .or_insert_with(|| Value::Object(Map::new()));
                let Some(bucket) = bucket.as_object_mut() else {
                    continue;
                };
                if let Some(default) = tool.get("default").and_then(Value::as_str) {
                    let action = match default {
                        "allow" => "allow",
                        "deny" => "deny",
                        _ => "ask",
                    };
                    bucket.insert("*".into(), Value::String(action.into()));
                }
                for (key, action) in [
                    ("always_allow", "allow"),
                    ("always_deny", "deny"),
                    ("always_confirm", "ask"),
                ] {
                    if let Some(Value::Array(entries)) = tool.get(key) {
                        for entry in entries {
                            if let Some(pattern) = entry.get("pattern").and_then(Value::as_str) {
                                bucket.insert(pattern.into(), Value::String(action.into()));
                            }
                        }
                    }
                }
            }
        }
        let mut result = Map::from_iter([("permission".into(), Value::Object(categories))]);
        if let Some(agent) = value.get("agent").and_then(Value::as_object) {
            let mut overrides = Map::new();
            for key in ["sandbox_permissions", "profiles"] {
                if let Some(value) = agent.get(key) {
                    overrides.insert(key.into(), value.clone());
                }
            }
            if !overrides.is_empty() {
                result.insert("zed".into(), Value::Object(overrides));
            }
        }
        return Value::Object(result);
    }
    if target == "hermesagent" {
        let mut bash = Map::new();
        if let Some(Value::Array(values)) = value.get("command_allowlist") {
            for pattern in values.iter().filter_map(Value::as_str) {
                bash.insert(pattern.into(), Value::String("allow".into()));
            }
        }
        if let Some(Value::Object(approvals)) = value.get("approvals") {
            if let Some(Value::Array(values)) = approvals.get("deny") {
                for pattern in values.iter().filter_map(Value::as_str) {
                    bash.insert(pattern.into(), Value::String("deny".into()));
                }
            }
        }
        let mut permission = Map::new();
        if !bash.is_empty() {
            permission.insert("bash".into(), Value::Object(bash));
        }
        if let Some(Value::Object(security)) = value.get("security") {
            if let Some(Value::Object(blocklist)) = security.get("website_blocklist") {
                if let Some(Value::Array(domains)) = blocklist.get("domains") {
                    let rules: Map<String, Value> = domains
                        .iter()
                        .filter_map(Value::as_str)
                        .map(|domain| (domain.into(), Value::String("deny".into())))
                        .collect();
                    if !rules.is_empty() {
                        permission.insert("webfetch".into(), Value::Object(rules));
                    }
                }
            }
        }
        return Value::Object(Map::from_iter([(
            "permission".into(),
            Value::Object(permission),
        )]));
    }
    if target == "kiro" || target == "kiro-cli" || target == "kiro-ide" {
        let mut categories = Map::new();
        if let Some(Value::Object(tools)) = value.get("toolsSettings") {
            for (category, key) in [
                ("bash", "shell"),
                ("read", "read"),
                ("write", "write"),
                ("grep", "grep"),
                ("glob", "glob"),
            ] {
                if let Some(Value::Object(settings)) = tools.get(key) {
                    let mut rules = Map::new();
                    if let Some(Value::Array(values)) = settings.get(if category == "bash" {
                        "allowedCommands"
                    } else {
                        "allowedPaths"
                    }) {
                        for pattern in values.iter().filter_map(Value::as_str) {
                            rules.insert(pattern.into(), Value::String("allow".into()));
                        }
                    }
                    if let Some(Value::Array(values)) = settings.get(if category == "bash" {
                        "deniedCommands"
                    } else {
                        "deniedPaths"
                    }) {
                        for pattern in values.iter().filter_map(Value::as_str) {
                            rules.insert(pattern.into(), Value::String("deny".into()));
                        }
                    }
                    if !rules.is_empty() {
                        categories.insert(category.into(), Value::Object(rules));
                    }
                }
            }
        }
        if let Some(Value::Array(tools)) = value.get("allowedTools") {
            for tool in tools.iter().filter_map(Value::as_str) {
                if tool == "web_fetch" {
                    categories.insert(
                        "webfetch".into(),
                        Value::Object(Map::from_iter([(
                            "*".into(),
                            Value::String("allow".into()),
                        )])),
                    );
                }
                if tool == "web_search" {
                    categories.insert(
                        "websearch".into(),
                        Value::Object(Map::from_iter([(
                            "*".into(),
                            Value::String("allow".into()),
                        )])),
                    );
                }
            }
        }
        return Value::Object(Map::from_iter([(
            "permission".into(),
            Value::Object(categories),
        )]));
    }
    if target == "kimi-code" {
        let mut categories = Map::new();
        if let Some(Value::Object(permission)) = value.get("permission") {
            if let Some(Value::Array(entries)) = permission.get("rules") {
                for entry in entries {
                    if let Some(object) = entry.as_object() {
                        let pattern = object.get("pattern").and_then(Value::as_str).unwrap_or("");
                        let action = object
                            .get("decision")
                            .and_then(Value::as_str)
                            .unwrap_or("ask");
                        if let Some((category, pattern)) =
                            parse_permission_entry(pattern, "reasonix")
                        {
                            let bucket = categories
                                .entry(category)
                                .or_insert_with(|| Value::Object(Map::new()));
                            if let Some(map) = bucket.as_object_mut() {
                                map.insert(pattern, Value::String(action.into()));
                            }
                        }
                    }
                }
            }
        }
        let mut result = Map::from_iter([("permission".into(), Value::Object(categories))]);
        let mut override_fields = Map::new();
        for key in ["default_permission_mode", "tools"] {
            if let Some(value) = value.get(key) {
                override_fields.insert(
                    if key == "default_permission_mode" {
                        "defaultPermissionMode"
                    } else {
                        key
                    }
                    .into(),
                    value.clone(),
                );
            }
        }
        if !override_fields.is_empty() {
            result.insert("kimi-code".into(), Value::Object(override_fields));
        }
        return Value::Object(result);
    }
    if target == "opencode" {
        let mut categories = Map::new();
        if let Some(Value::Object(permission)) = value.get("permission") {
            for (key, raw) in permission {
                let category = if key == "task" { "agent" } else { key };
                let mut rules = Map::new();
                match raw {
                    Value::String(action) => {
                        rules.insert("*".into(), Value::String(action.clone()));
                    }
                    Value::Object(map) => {
                        for (pattern, action) in map {
                            rules.insert(pattern.clone(), action.clone());
                        }
                    }
                    _ => {}
                }
                categories.insert(category.into(), Value::Object(rules));
            }
        }
        return Value::Object(Map::from_iter([(
            "permission".into(),
            Value::Object(categories),
        )]));
    }
    if target == "rovodev" {
        let mut categories = Map::new();
        let mut set_rule = |category: &str, pattern: &str, action: &str| {
            categories
                .entry(category)
                .or_insert_with(|| Value::Object(Map::new()))
                .as_object_mut()
                .expect("Rovo permission category is an object")
                .insert(pattern.into(), Value::String(action.into()));
        };
        if let Some(Value::Object(tool_permissions)) = value.get("toolPermissions") {
            if let Some(action) = tool_permissions.get("default").and_then(Value::as_str) {
                set_rule("*", "*", action);
            }
            if let Some(Value::Object(bash)) = tool_permissions.get("bash") {
                if let Some(action) = bash.get("default").and_then(Value::as_str) {
                    set_rule("bash", "*", action);
                }
                if let Some(Value::Array(commands)) = bash.get("commands") {
                    for command in commands {
                        let Some(command) = command.as_object() else {
                            continue;
                        };
                        let Some(pattern) = command.get("command").and_then(Value::as_str) else {
                            continue;
                        };
                        let Some(action) = command.get("permission").and_then(Value::as_str) else {
                            continue;
                        };
                        set_rule("bash", pattern, action);
                    }
                }
            }
            let tool_category = |tool: &str| match tool {
                "open_files" | "expand_code_chunks" | "expand_folder" | "grep" | "getJiraIssue"
                | "getConfluencePage" => Some("read"),
                "find_and_replace_code"
                | "create_file"
                | "delete_file"
                | "move_file"
                | "createTechnicalPlan"
                | "createJiraIssue"
                | "updateJiraIssue"
                | "createConfluencePage"
                | "updateConfluencePage" => Some("edit"),
                _ => None,
            };
            if let Some(Value::Object(tools)) = tool_permissions.get("tools") {
                for (tool, action) in tools {
                    let Some(category) = tool_category(tool) else {
                        continue;
                    };
                    let Some(action) = action.as_str() else {
                        continue;
                    };
                    set_rule(category, "*", action);
                }
            }
            if let Some(Value::Array(paths)) = tool_permissions.get("allowedExternalPaths") {
                for path in paths.iter().filter_map(Value::as_str) {
                    set_rule("read", path, "allow");
                }
            }
        }
        return Value::Object(Map::from_iter([(
            "permission".into(),
            Value::Object(categories),
        )]));
    }
    if target == "goose" {
        let mut categories = Map::new();
        if let Some(Value::Object(user)) = value.get("user") {
            for (key, action) in [
                ("always_allow", "allow"),
                ("ask_before", "ask"),
                ("never_allow", "deny"),
            ] {
                if let Some(Value::Array(entries)) = user.get(key) {
                    for entry in entries.iter().filter_map(Value::as_str) {
                        let category = match entry {
                            "developer__shell" => "bash",
                            "developer__text_editor" => "edit",
                            other => other,
                        };
                        let bucket = categories
                            .entry(category)
                            .or_insert_with(|| Value::Object(Map::new()));
                        if let Some(map) = bucket.as_object_mut() {
                            map.insert("*".into(), Value::String(action.into()));
                        }
                    }
                }
            }
        }
        return Value::Object(Map::from_iter([(
            "permission".into(),
            Value::Object(categories),
        )]));
    }
    if target == "vibe" {
        let mut categories = Map::new();
        if let Some(Value::Object(tools)) = value.get("tools") {
            for (tool, raw) in tools {
                if let Some(tool_config) = raw.as_object() {
                    let category = match tool.as_str() {
                        "read_file" => "read",
                        "write_file" => "write",
                        "web_fetch" => "webfetch",
                        "web_search" => "websearch",
                        "task" => "agent",
                        other => other,
                    };
                    let mut rules = Map::new();
                    if let Some(action) = tool_config.get("permission").and_then(Value::as_str) {
                        rules.insert(
                            "*".into(),
                            Value::String(
                                match action {
                                    "always" => "allow",
                                    "never" => "deny",
                                    _ => "ask",
                                }
                                .into(),
                            ),
                        );
                    }
                    for (key, action) in [("allowlist", "allow"), ("denylist", "deny")] {
                        if let Some(Value::Array(values)) = tool_config.get(key) {
                            for pattern in values.iter().filter_map(Value::as_str) {
                                rules.insert(pattern.into(), Value::String(action.into()));
                            }
                        }
                    }
                    if !rules.is_empty() {
                        categories.insert(category.into(), Value::Object(rules));
                    }
                }
            }
        }
        return Value::Object(Map::from_iter([(
            "permission".into(),
            Value::Object(categories),
        )]));
    }
    if target == "warp" {
        let mut bash = Map::new();
        if let Some(Value::Object(agents)) = value.get("agents") {
            if let Some(Value::Object(profiles)) = agents.get("profiles") {
                if let Some(Value::Array(values)) =
                    profiles.get("agent_mode_command_execution_allowlist")
                {
                    for pattern in values.iter().filter_map(Value::as_str) {
                        bash.insert(pattern.into(), Value::String("allow".into()));
                    }
                }
                if let Some(Value::Array(values)) =
                    profiles.get("agent_mode_command_execution_denylist")
                {
                    for pattern in values.iter().filter_map(Value::as_str) {
                        bash.insert(pattern.into(), Value::String("deny".into()));
                    }
                }
            }
        }
        return Value::Object(Map::from_iter([(
            "permission".into(),
            Value::Object(Map::from_iter([("bash".into(), Value::Object(bash))])),
        )]));
    }
    if target == "copilot" {
        let mut permission = Map::new();
        for (category, key) in [
            ("bash", "chat.tools.terminal.autoApprove"),
            ("edit", "chat.tools.edits.autoApprove"),
            ("webfetch", "chat.tools.urls.autoApprove"),
        ] {
            if let Some(Value::Object(entries)) = value.get(key) {
                let mut rules = Map::new();
                for (pattern, flag) in entries {
                    if let Some(action) = flag
                        .as_bool()
                        .map(|allowed| if allowed { "allow" } else { "deny" })
                    {
                        rules.insert(pattern.clone(), Value::String(action.into()));
                    }
                }
                if !rules.is_empty() {
                    permission.insert(category.into(), Value::Object(rules));
                }
            }
        }
        return Value::Object(Map::from_iter([(
            "permission".into(),
            Value::Object(permission),
        )]));
    }
    if target == "copilotcli" {
        let mut rules = Map::new();
        if let Some(Value::Array(entries)) = value.get("allowedUrls") {
            for pattern in entries.iter().filter_map(Value::as_str) {
                rules.insert(pattern.into(), Value::String("allow".into()));
            }
        }
        if let Some(Value::Array(entries)) = value.get("deniedUrls") {
            for pattern in entries.iter().filter_map(Value::as_str) {
                rules.insert(pattern.into(), Value::String("deny".into()));
            }
        }
        return Value::Object(Map::from_iter([(
            "permission".into(),
            Value::Object(Map::from_iter([("webfetch".into(), Value::Object(rules))])),
        )]));
    }
    if target == "zoocode" {
        let mut rules = Map::new();
        if let Some(Value::Array(entries)) = value.get("zoo-code.allowedCommands") {
            for pattern in entries.iter().filter_map(Value::as_str) {
                rules.insert(pattern.into(), Value::String("allow".into()));
            }
        }
        if let Some(Value::Array(entries)) = value.get("zoo-code.deniedCommands") {
            for pattern in entries.iter().filter_map(Value::as_str) {
                rules.insert(pattern.into(), Value::String("deny".into()));
            }
        }
        return Value::Object(Map::from_iter([(
            "permission".into(),
            Value::Object(Map::from_iter([("bash".into(), Value::Object(rules))])),
        )]));
    }
    if value.get("permission").is_some() {
        return value.clone();
    }
    Value::Object(Map::from_iter([("permission".into(), value.clone())]))
}

fn import_takt_permissions(value: &Value) -> Value {
    let provider = value
        .get("provider")
        .and_then(Value::as_str)
        .unwrap_or("claude");
    let profile = value
        .get("provider_profiles")
        .and_then(|profiles| profiles.get(provider))
        .and_then(Value::as_object);
    let mode = profile
        .and_then(|profile| profile.get("default_permission_mode"))
        .and_then(Value::as_str)
        .unwrap_or("readonly");
    let action = if mode == "full" || mode == "edit" {
        "allow"
    } else {
        "deny"
    };
    let category = if mode == "edit" { "edit" } else { "bash" };
    let mut permission = Map::new();
    permission.insert(
        category.into(),
        Value::Object(Map::from_iter([("*".into(), Value::String(action.into()))])),
    );
    let mut result = Map::from_iter([("permission".into(), Value::Object(permission))]);
    let mut override_fields = Map::new();
    if let Some(profile) = profile {
        if let Some(value) = profile.get("step_permission_overrides") {
            override_fields.insert("step_permission_overrides".into(), value.clone());
        }
    }
    for key in [
        "provider_options",
        "network_policy",
        "filesystem_policy",
        "shell_policy",
        "workflow_command_gates",
    ] {
        if let Some(value) = value.get(key) {
            override_fields.insert(key.into(), value.clone());
        }
    }
    if !override_fields.is_empty() {
        result.insert("takt".into(), Value::Object(override_fields));
    }
    Value::Object(result)
}

fn import_ignore_value(value: &Value, path: &Path, target: &str) -> Result<String> {
    if let Some(values) = value.get("private_files").and_then(Value::as_array) {
        return Ok(values
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join("\n"));
    }
    if let Some(values) = value
        .get("permissions")
        .and_then(|permissions| permissions.get("deny"))
        .and_then(Value::as_array)
    {
        let reasonix = target == "reasonix";
        return Ok(values
            .iter()
            .filter_map(|entry| {
                let entry = entry.as_str()?;
                if let Some(pattern) = entry
                    .strip_prefix("Read(")
                    .and_then(|value| value.strip_suffix(')'))
                    .filter(|value| !value.is_empty())
                {
                    return Some(pattern.to_owned());
                }
                (!reasonix && !entry.is_empty()).then(|| entry.to_owned())
            })
            .collect::<Vec<_>>()
            .join("\n"));
    }
    Ok(fs::read_to_string(path).unwrap_or_default())
}

fn internal_has_diff(result: &InternalResults) -> bool {
    result.rules.count > 0
        || result.ignore.count > 0
        || result.mcp.count > 0
        || result.commands.count > 0
        || result.subagents.count > 0
        || result.skills.count > 0
        || result.hooks.count > 0
        || result.permissions.count > 0
        || result.checks.count > 0
        || result.activation.count > 0
}

fn other_target_owns_location(
    config: &Config,
    current: &TargetSpec,
    feature: Feature,
    location: &str,
) -> bool {
    if feature == Feature::Rules && current.name == "pi" && location == "AGENTS.md" {
        return true;
    }
    if feature == Feature::Subagents
        && matches!(current.name.as_str(), "kiro" | "kiro-cli" | "kiro-ide")
        && location == ".kiro/agents"
        && config.targets().iter().any(|target| {
            target != &current.name && matches!(target.as_str(), "kiro" | "kiro-cli" | "kiro-ide")
        })
    {
        return true;
    }
    for target in config.targets() {
        if target == &current.name {
            continue;
        }
        if target == "rovodev"
            && feature == Feature::Rules
            && !config.global()
            && location == "AGENTS.md"
        {
            return true;
        }
        let Some(other) = target_spec(target) else {
            continue;
        };
        if !config
            .features_for(target)
            .iter()
            .any(|value| value == feature.as_str())
            || !other.supports(feature, config.global(), false)
        {
            continue;
        }
        let paths = other.paths(config.global());
        let matches = match feature {
            Feature::Rules => {
                paths
                    .root_rule
                    .as_ref()
                    .map(|path| path.path() == location)
                    .unwrap_or(false)
                    || paths
                        .local_rule
                        .as_ref()
                        .map(|path| path.path() == location)
                        .unwrap_or(false)
                    || paths
                        .nonroot_rule_dir
                        .as_deref()
                        .map(|path| path == location)
                        .unwrap_or(false)
            }
            Feature::Commands => paths.command_dir.as_deref() == Some(location),
            Feature::Subagents => {
                if paths.aggregate_subagents {
                    location == ".roomodes"
                } else {
                    paths.subagent_dir.as_deref() == Some(location)
                }
            }
            Feature::Skills => paths.skill_dir.as_deref() == Some(location),
            Feature::Checks => paths
                .checks
                .as_ref()
                .map(|path| path.file == "__dynamic__" && path.dir == location)
                .unwrap_or(false),
            Feature::Ignore | Feature::Mcp | Feature::Hooks | Feature::Permissions => false,
        };
        if matches {
            return true;
        }
    }
    false
}

fn shared_feature_owns_path(
    model: &CanonicalModel,
    spec: &TargetSpec,
    feature: Feature,
    global: bool,
    output_root: &Path,
    relative_path: &str,
) -> bool {
    let paths = spec.paths(global);
    let root_for = |candidate: Option<&String>| {
        candidate.and_then(|root| {
            let prefix = format!("{root}/");
            relative_path
                .strip_prefix(&prefix)
                .and_then(|relative| relative.split('/').next())
        })
    };
    match (spec.name.as_str(), feature) {
        ("devin" | "warp", Feature::Skills) => {
            let Some(name) = root_for(paths.skill_dir.as_ref()) else {
                return false;
            };
            model
                .commands
                .iter()
                .filter(|command| command.targeted_at(&spec.name))
                .any(|command| command_slug(&command.relative_path) == name)
        }
        ("reasonix", Feature::Skills | Feature::Subagents) => {
            let Some(content) = fs::read_to_string(output_root.join(relative_path)).ok() else {
                return false;
            };
            let Ok(parsed) = parse_frontmatter(&content, &output_root.join(relative_path)) else {
                return false;
            };
            let is_subagent = parsed.data.get("runAs").and_then(Value::as_str) == Some("subagent");
            match feature {
                Feature::Skills => is_subagent,
                Feature::Subagents => !is_subagent,
                _ => false,
            }
        }
        _ => false,
    }
}

fn remove_hermes_plugin_registration(
    config: &Config,
    model: &CanonicalModel,
    spec: &TargetSpec,
    output_root: &Path,
    feature: Feature,
    result: &mut FeatureResult,
) -> Result<()> {
    if spec.name != "hermesagent" || !matches!(feature, Feature::Commands | Feature::Subagents) {
        return Ok(());
    }
    let plugin_root = output_root.join(hermes_plugin_dir(config.global(), feature.as_str()));
    if fs::read_to_string(plugin_root.join(".carabiner-owned"))
        .ok()
        .as_deref()
        != Some("Generated and owned by Carabiner.\n")
    {
        return Ok(());
    }
    let has_sources = match feature {
        Feature::Commands => model
            .commands
            .iter()
            .any(|command| command.targeted_at("hermesagent")),
        Feature::Subagents => model
            .subagents
            .iter()
            .any(|agent| agent.targeted_at("hermesagent")),
        _ => false,
    };
    if has_sources {
        return Ok(());
    }
    let relative = if config.global() {
        spec.paths(config.global())
            .mcp
            .as_ref()
            .map(|path| path.path())
    } else {
        Some(".hermes/config.yaml".to_owned())
    };
    let Some(relative) = relative else {
        return Ok(());
    };
    let path = output_root.join(relative);
    let Ok(existing) = read_structured_file(&path) else {
        return Ok(());
    };
    let plugin = format!("carabiner-{}", feature.as_str());
    let mut document = existing.as_object().cloned().unwrap_or_default();
    let mut changed = false;
    if let Some(Value::Object(plugins)) = document.get_mut("plugins") {
        if let Some(Value::Array(enabled)) = plugins.get_mut("enabled") {
            let before = enabled.len();
            enabled.retain(|value| value.as_str() != Some(plugin.as_str()));
            changed = enabled.len() != before;
            if enabled.is_empty() {
                plugins.remove("enabled");
            }
        }
        if plugins.is_empty() {
            document.remove("plugins");
        }
    }
    if !changed {
        return Ok(());
    }
    let content = serialize_json_or_yaml(&Value::Object(document), "config.yaml")?;
    if !config.preview() {
        write_text(&path, &content, false)?;
    }
    result.count += 1;
    result.paths.push(relative_slash(output_root, &path));
    Ok(())
}

fn delete_orphans(
    config: &Config,
    model: &CanonicalModel,
    output_root: &Path,
    spec: &TargetSpec,
    feature: Feature,
    generated: &HashSet<String>,
    result: &mut FeatureResult,
) -> Result<()> {
    if !config.delete() {
        return Ok(());
    }
    remove_hermes_plugin_registration(config, model, spec, output_root, feature, result)?;
    // Devin and Warp commands are skills, so their command feature does not
    // own deletions in the shared skills tree. The skills feature handles it.
    if matches!(spec.name.as_str(), "devin" | "warp") && feature == Feature::Commands {
        return Ok(());
    }
    let paths = spec.paths(config.global());
    let mut candidates = Vec::new();
    let mut roots = Vec::new();
    match feature {
        Feature::Rules => {
            if let Some(root) = &paths.root_rule {
                let candidate = root.path();
                if !(spec.name == "pi" && !config.global() && candidate == "AGENTS.md") {
                    candidates.push(candidate);
                }
                if spec.name == "pi" {
                    let override_path = if root.dir == "." {
                        "AGENTS.override.md".into()
                    } else {
                        format!("{}/AGENTS.override.md", root.dir.trim_end_matches('/'))
                    };
                    candidates.push(override_path);
                    candidates.push(pi_append_path(config.global()));
                }
            }
            if let Some(local) = &paths.local_rule {
                candidates.push(local.path());
            }
            if let Some(dir) = &paths.nonroot_rule_dir {
                roots.push(dir.clone());
            }
        }
        Feature::Commands => {
            if let Some(dir) = &paths.command_dir {
                roots.push(dir.clone());
            }
            if spec.name == "hermesagent" {
                roots.push(hermes_plugin_dir(config.global(), "commands"));
            }
        }
        Feature::Subagents => {
            if paths.aggregate_subagents {
                candidates.push(".roomodes".into());
            } else if let Some(dir) = &paths.subagent_dir {
                roots.push(dir.clone());
            }
            if spec.name == "hermesagent" {
                roots.push(hermes_plugin_dir(config.global(), "subagents"));
            }
            if spec.name == "vibe" {
                roots.push(".vibe/prompts".into());
            }
        }
        Feature::Skills => {
            if let Some(dir) = &paths.skill_dir {
                roots.push(dir.clone());
            }
        }
        Feature::Checks => {
            if let Some(path) = &paths.checks {
                if path.file == "__dynamic__" {
                    roots.push(path.dir.clone());
                } else if !is_shared_config_for_path(path) {
                    candidates.push(path.path());
                }
            }
        }
        Feature::Hooks => {
            if spec.name == "cline" {
                if let Some(path) = &paths.hooks {
                    roots.push(path.dir.clone());
                }
            }
        }
        Feature::Permissions => {
            if spec.name == "codexcli" {
                candidates.push(".codex/rules/carabiner.rules".into());
            }
        }
        Feature::Ignore => {
            if spec.name == "hermesagent" {
                roots.push(hermes_plugin_dir(config.global(), "ignore"));
            } else if !matches!(
                spec.name.as_str(),
                "claudecode" | "claudecode-legacy" | "zed" | "reasonix"
            ) {
                if let Some(path) = &paths.ignore {
                    candidates.push(path.path());
                }
            }
        }
        Feature::Mcp => {}
    }

    for candidate in candidates {
        if generated.contains(&candidate)
            || other_target_owns_location(config, spec, feature, &candidate)
        {
            continue;
        }
        let path = output_root.join(&candidate);
        assert_output_path_safe(output_root, &candidate)?;
        if path.is_file() {
            if !config.preview() {
                fs::remove_file(&path)?;
            }
            result.paths.push(candidate);
            result.count += 1;
        }
    }
    for root in roots {
        if other_target_owns_location(config, spec, feature, &root) {
            continue;
        }
        let base = output_root.join(&root);
        if spec.name == "hermesagent"
            && root == hermes_plugin_dir(config.global(), feature.as_str())
            && matches!(
                feature,
                Feature::Commands | Feature::Subagents | Feature::Ignore | Feature::Checks
            )
            && if feature == Feature::Subagents {
                fs::read_to_string(base.join("plugin.yaml"))
                    .ok()
                    .is_none_or(|content| !content.starts_with("name: carabiner-subagents\n"))
            } else {
                fs::read_to_string(base.join(".carabiner-owned"))
                    .ok()
                    .as_deref()
                    != Some("Generated and owned by Carabiner.\n")
            }
        {
            continue;
        }
        if fs::symlink_metadata(&base)
            .map(|metadata| metadata.file_type().is_symlink())
            .unwrap_or(false)
        {
            return Err(anyhow!(
                "Refusing to delete through symlinked managed directory {}",
                base.display()
            ));
        }
        for path in walk_files(&base) {
            let relative = relative_slash(output_root, &path);
            if feature == Feature::Subagents
                && matches!(spec.name.as_str(), "kiro" | "kiro-cli" | "kiro-ide")
                && (relative == ".kiro/agents/default.json"
                    || relative.ends_with("/.kiro/agents/default.json"))
            {
                continue;
            }
            if feature == Feature::Rules
                && spec.name == "cline"
                && (relative == ".clinerules/hooks"
                    || relative.starts_with(".clinerules/hooks/")
                    || relative == ".clinerules/workflows"
                    || relative.starts_with(".clinerules/workflows/"))
            {
                continue;
            }
            if generated.contains(&relative)
                || shared_feature_owns_path(
                    model,
                    spec,
                    feature,
                    config.global(),
                    output_root,
                    &relative,
                )
            {
                continue;
            }
            if feature == Feature::Hooks
                && spec.name == "cline"
                && !fs::read_to_string(&path)
                    .map(|content| content.contains("carabiner-owned: cline-hooks"))
                    .unwrap_or(false)
            {
                continue;
            }
            assert_output_path_safe(output_root, &relative)?;
            if !config.preview() {
                fs::remove_file(&path)?;
            }
            result.paths.push(relative);
            result.count += 1;
        }
    }
    Ok(())
}

fn effective_hooks(source: &Value, target: &str) -> Value {
    let Some(root) = source.as_object() else {
        return Value::Object(Map::new());
    };
    let mut hooks = root
        .get("hooks")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let key = if matches!(target, "kiro" | "kiro-cli" | "kiro-ide") {
        "kiro"
    } else {
        target
    };

    if let Some(Value::Object(block)) = root.get(key) {
        if let Some(Value::Object(overrides)) = block.get("hooks") {
            for (event, value) in overrides {
                hooks.insert(event.clone(), value.clone());
            }
        }
    }
    Value::Object(hooks)
}

fn valid_opencode_command_entry(entry: &Map<String, Value>) -> bool {
    entry.get("description").is_none_or(Value::is_string)
        && entry.get("agent").is_none_or(Value::is_string)
        && entry.get("model").is_none_or(Value::is_string)
        && entry.get("subtask").is_none_or(Value::is_boolean)
}

fn valid_opencode_agent_entry(entry: &Map<String, Value>) -> bool {
    entry.get("description").is_none_or(Value::is_string)
        && entry.get("mode").is_none_or(Value::is_string)
        && entry.get("name").is_none_or(Value::is_string)
        && entry.get("model").is_none_or(Value::is_string)
        && entry.get("temperature").is_none_or(Value::is_number)
        && entry.get("top_p").is_none_or(Value::is_number)
        && entry.get("prompt").is_none_or(Value::is_string)
        && entry.get("disable").is_none_or(Value::is_boolean)
        && entry.get("tools").is_none_or(Value::is_object)
        && entry
            .get("permission")
            .is_none_or(|value| value.is_string() || value.is_object())
}

fn read_opencode_config_with_path(output_root: &Path, global: bool) -> Option<(Value, PathBuf)> {
    let relative = if global {
        ".config/opencode/opencode.json"
    } else {
        "opencode.json"
    };
    let primary = output_root.join(relative);
    let alternate = if relative.ends_with(".json") {
        output_root.join(format!("{relative}c"))
    } else {
        output_root.join(relative)
    };
    for path in [primary, alternate] {
        if path.is_file() {
            if let Ok(value) = read_structured_file(&path) {
                return Some((value, path));
            }
        }
    }
    None
}

fn resolve_opencode_file_template(value: &str, config_path: &Path) -> String {
    let trimmed = value.trim();
    let Some(reference) = trimmed
        .strip_prefix("{file:")
        .and_then(|reference| reference.strip_suffix('}'))
    else {
        return value.to_owned();
    };
    let reference = reference
        .trim()
        .strip_prefix("./")
        .unwrap_or(reference.trim());
    if reference.is_empty() {
        return value.to_owned();
    }
    let config_dir = config_path.parent().unwrap_or(Path::new("."));
    fs::read_to_string(config_dir.join(reference)).unwrap_or_else(|_| value.to_owned())
}

fn resolve_json_variant(
    output_root: &Path,
    path: &crate::targets::PathSpec,
) -> crate::targets::PathSpec {
    if !path.file.ends_with(".json") {
        return path.clone();
    }
    let jsonc_file = format!("{}c", path.file);
    let jsonc = crate::targets::PathSpec::new(path.dir.clone(), jsonc_file);
    if output_root.join(jsonc.path()).is_file()
        || (matches!(path.file.as_str(), "opencode.json" | "kilo.json")
            && !output_root.join(path.path()).is_file())
    {
        jsonc
    } else {
        path.clone()
    }
}
