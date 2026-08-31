use crate::model::{Feature, FeatureResult};
use crate::targets::{all_features, all_targets, VERSION};
use crate::util::{home_dir, parse_jsonc};
use anyhow::{anyhow, Context, Result};
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
type ResolvedOutputRoots = (Option<Vec<PathBuf>>, HashMap<String, Vec<PathBuf>>);

#[derive(Debug, Clone, Default)]
pub struct ConfigOptions {
    pub cwd: Option<PathBuf>,
    pub config_path: Option<String>,
    pub targets: Option<Vec<String>>,
    pub features: Option<Vec<String>>,
    pub output_roots: Option<Vec<String>>,
    pub delete: Option<bool>,
    pub global: Option<bool>,
    pub verbose: Option<bool>,
    pub silent: Option<bool>,
    pub simulate_commands: Option<bool>,
    pub simulate_subagents: Option<bool>,
    pub simulate_skills: Option<bool>,
    pub dry_run: Option<bool>,
    pub check: Option<bool>,
    pub flattened_command_naming: Option<String>,
    pub gitignore_targets_only: Option<bool>,
    pub gitignore_destination: Option<String>,
    pub input_root: Option<String>,
    pub input_roots: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
enum TargetFeatureValue {
    Array(Vec<String>),
    Object(Map<String, Value>),
}

#[derive(Debug, Clone)]
pub struct Config {
    cwd: PathBuf,
    config_path: PathBuf,
    config_exists: bool,
    targets: Vec<String>,
    config_file_targets: Vec<String>,
    wildcard_targets: bool,
    features: Vec<String>,
    target_features: HashMap<String, TargetFeatureValue>,
    targets_object: bool,
    output_roots: Option<Vec<PathBuf>>,
    output_roots_by_target: HashMap<String, Vec<PathBuf>>,
    input_roots: Vec<PathBuf>,
    delete: bool,
    global: bool,
    verbose: bool,
    silent: bool,
    simulate_commands: bool,
    simulate_subagents: bool,
    simulate_skills: bool,
    dry_run: bool,
    check: bool,
    gitignore_destination: String,
    gitignore_targets_only: bool,
    flattened_command_naming: String,
    sources: Vec<Value>,
}

impl Config {
    pub fn resolve(options: &ConfigOptions) -> Result<Self> {
        let cwd = options
            .cwd
            .clone()
            .unwrap_or_else(|| std::env::current_dir().expect("current directory is available"));
        let cwd = absolute(&cwd)?;

        if options.input_root.is_some() && options.input_roots.is_some() {
            return Err(anyhow!(
                "Invalid config: 'inputRoot' and 'inputRoots' cannot be combined."
            ));
        }
        let config_path = resolve_config_path(
            &cwd,
            options.config_path.as_deref(),
            options.input_root.as_deref(),
        )?;
        let (base, base_exists) = load_config_file(&config_path)?;
        validate_config_fields(&base, &config_path)?;
        let local_path = config_path
            .parent()
            .unwrap_or(Path::new("."))
            .join("carabiner.local.jsonc");
        let (local, local_exists) = load_config_file(&local_path)?;
        validate_config_fields(&local, &local_path)?;
        let config_exists = base_exists || local_exists;
        let merged = merge_objects(base, local);

        if merged.get("targets").and_then(Value::as_object).is_some()
            && (merged.get("features").is_some() || options.features.is_some())
        {
            return Err(anyhow!("Invalid config: when 'targets' is in object form, 'features' must be omitted. Declare per-target features inside the 'targets' object instead."));
        }

        let file_targets = merged.get("targets").cloned();
        let config_file_targets = parse_targets(file_targets.as_ref())?;
        let wildcard_targets = options
            .targets
            .as_ref()
            .map(|targets| targets.iter().any(|target| target == "*"))
            .unwrap_or_else(|| {
                file_targets
                    .as_ref()
                    .and_then(Value::as_array)
                    .is_some_and(|targets| {
                        targets.iter().any(|target| target.as_str() == Some("*"))
                    })
            });
        let (targets, target_features, targets_object) = if let Some(cli_targets) = &options.targets
        {
            (
                validate_targets(cli_targets.clone())?,
                HashMap::new(),
                false,
            )
        } else if let Some(targets_value) = file_targets.as_ref() {
            match targets_value {
                Value::Object(entries) => {
                    let mut values = HashMap::new();
                    for (name, value) in entries {
                        if name == "*" {
                            return Err(anyhow!("Invalid target '*' in object form: use the array form `targets: ['*']`."));
                        }
                        validate_target(name)?;
                        let feature_value = match value {
                            Value::Array(items) => {
                                let features = items
                                    .iter()
                                    .map(|item| {
                                        item.as_str()
                                            .ok_or_else(|| {
                                                anyhow!(
                                                    "features for target '{name}' must be strings"
                                                )
                                            })
                                            .map(ToOwned::to_owned)
                                    })
                                    .collect::<Result<Vec<_>>>()?;
                                TargetFeatureValue::Array(normalize_features(features)?)
                            }
                            Value::Object(object) => {
                                for (feature, feature_value) in object {
                                    let valid = feature_value.is_boolean()
                                        || feature_value.is_object()
                                        || matches!(
                                            feature_value.as_str(),
                                            Some("gitignore") | Some("gitattributes")
                                        );
                                    if !valid {
                                        return Err(anyhow!(
                                            "feature '{feature}' for target '{name}' must be a boolean, options object, or gitignore destination"
                                        ));
                                    }
                                }
                                TargetFeatureValue::Object(object.clone())
                            }
                            _ => {
                                return Err(anyhow!(
                                    "target '{name}' must contain an array or object of features"
                                ))
                            }
                        };
                        values.insert(name.clone(), feature_value);
                    }
                    (entries.keys().cloned().collect(), values, true)
                }
                _ => (parse_targets(Some(targets_value))?, HashMap::new(), false),
            }
        } else {
            (vec!["agentsmd".into()], HashMap::new(), false)
        };

        let file_features = parse_string_array(merged.get("features"))?;
        let features = if targets_object && options.features.is_none() {
            Vec::new()
        } else if let Some(cli_features) = &options.features {
            normalize_features(cli_features.clone())?
        } else if let Some(file_features) = file_features {
            normalize_features(file_features)?
        } else {
            vec!["rules".into()]
        };

        let has_configured_input_root = options.input_root.is_some()
            || options.input_roots.is_some()
            || merged.get("inputRoot").is_some()
            || merged.get("inputRoots").is_some();
        let global = options
            .global
            .or_else(|| {
                if has_configured_input_root {
                    None
                } else {
                    merged.get("global").and_then(Value::as_bool)
                }
            })
            .unwrap_or(false);
        let output_value = options
            .output_roots
            .as_ref()
            .map(|roots| {
                Value::Array(
                    roots
                        .iter()
                        .map(|root| Value::String(root.clone()))
                        .collect(),
                )
            })
            .or_else(|| merged.get("outputRoots").cloned())
            .unwrap_or_else(|| Value::Array(vec![Value::String(".".into())]));
        let (output_roots, output_roots_by_target) =
            resolve_output_roots(&cwd, &output_value, global)?;

        let input_roots = resolve_input_roots(&cwd, options, &merged)?;
        if input_roots.is_empty() {
            return Err(anyhow!("input roots must not be empty"));
        }

        let delete = options
            .delete
            .or_else(|| merged.get("delete").and_then(Value::as_bool))
            .unwrap_or(false);
        let verbose = options
            .verbose
            .or_else(|| merged.get("verbose").and_then(Value::as_bool))
            .unwrap_or(false);
        let silent = options
            .silent
            .or_else(|| merged.get("silent").and_then(Value::as_bool))
            .unwrap_or(false);
        let simulate_commands = options
            .simulate_commands
            .or_else(|| merged.get("simulateCommands").and_then(Value::as_bool))
            .unwrap_or(false);
        let simulate_subagents = options
            .simulate_subagents
            .or_else(|| merged.get("simulateSubagents").and_then(Value::as_bool))
            .unwrap_or(false);
        let simulate_skills = options
            .simulate_skills
            .or_else(|| merged.get("simulateSkills").and_then(Value::as_bool))
            .unwrap_or(false);
        let dry_run = options
            .dry_run
            .or_else(|| merged.get("dryRun").and_then(Value::as_bool))
            .unwrap_or(false);
        let check = options
            .check
            .or_else(|| merged.get("check").and_then(Value::as_bool))
            .unwrap_or(false);
        if dry_run && check {
            return Err(anyhow!("--dry-run and --check cannot be used together"));
        }

        let gitignore_destination = options
            .gitignore_destination
            .clone()
            .or_else(|| {
                merged
                    .get("gitignoreDestination")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            })
            .unwrap_or_else(|| "gitignore".into());
        if !matches!(
            gitignore_destination.as_str(),
            "gitignore" | "gitattributes"
        ) {
            return Err(anyhow!(
                "Invalid gitignoreDestination '{gitignore_destination}'. Expected 'gitignore' or 'gitattributes'."
            ));
        }
        let gitignore_targets_only = options
            .gitignore_targets_only
            .or_else(|| merged.get("gitignoreTargetsOnly").and_then(Value::as_bool))
            .unwrap_or(true);

        let flattened_command_naming = options
            .flattened_command_naming
            .clone()
            .or_else(|| {
                merged
                    .get("flattenedCommandNaming")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            })
            .unwrap_or_else(|| "basename".into());
        if !matches!(flattened_command_naming.as_str(), "basename" | "path") {
            return Err(anyhow!(
                "Invalid flattenedCommandNaming '{flattened_command_naming}'. Expected 'basename' or 'path'."
            ));
        }
        let sources = merged
            .get("sources")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        validate_conflicting_targets(&targets)?;
        Ok(Self {
            cwd,
            config_path,
            config_exists,
            targets,
            config_file_targets,
            wildcard_targets,
            features,
            target_features,
            targets_object,
            output_roots,
            output_roots_by_target,
            input_roots,
            delete,
            global,
            verbose,
            silent,
            simulate_commands,
            simulate_subagents,
            simulate_skills,
            dry_run,
            check,
            gitignore_destination,
            gitignore_targets_only,
            flattened_command_naming,
            sources,
        })
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }
    pub fn config_path(&self) -> &Path {
        &self.config_path
    }
    pub fn config_exists(&self) -> bool {
        self.config_exists
    }
    pub fn sources(&self) -> &[Value] {
        &self.sources
    }
    pub fn targets(&self) -> &[String] {
        &self.targets
    }
    pub fn config_file_targets(&self) -> &[String] {
        &self.config_file_targets
    }
    pub fn wildcard_targets(&self) -> bool {
        self.wildcard_targets
    }
    pub fn features(&self) -> Vec<String> {
        if self.targets_object {
            let mut result = Vec::new();
            for target in &self.targets {
                for feature in self.features_for(target) {
                    if !result.contains(&feature) {
                        result.push(feature);
                    }
                }
            }
            result
        } else {
            self.features.clone()
        }
    }
    pub fn features_for(&self, target: &str) -> Vec<String> {
        if !self.targets_object {
            return self.features.clone();
        }
        match self.target_features.get(target) {
            Some(TargetFeatureValue::Array(items)) => {
                normalize_features(items.clone()).unwrap_or_default()
            }
            Some(TargetFeatureValue::Object(object)) => {
                if value_enabled(object.get("*")) {
                    return all_features();
                }
                object
                    .iter()
                    .filter_map(|(key, value)| {
                        if key == "gitignoreDestination" || !value_enabled(Some(value)) {
                            return None;
                        }
                        if all_features().iter().any(|feature| feature == key) {
                            Some(key.clone())
                        } else {
                            None
                        }
                    })
                    .collect()
            }
            None => Vec::new(),
        }
    }
    pub fn feature_options(&self, target: &str, feature: &str) -> Option<Map<String, Value>> {
        if !self.targets_object {
            return None;
        }
        let TargetFeatureValue::Object(object) = self.target_features.get(target)? else {
            return None;
        };
        object.get(feature)?.as_object().cloned()
    }
    pub fn gitignore_destination(&self, target: &str, feature: Option<&str>) -> String {
        let root = self.gitignore_destination.as_str();
        if !self.targets_object {
            return root.into();
        }
        let Some(TargetFeatureValue::Object(object)) = self.target_features.get(target) else {
            return root.into();
        };
        if let Some(feature) = feature {
            if let Some(Value::Object(feature_object)) = object.get(feature) {
                if feature_object
                    .get("gitignoreDestination")
                    .and_then(Value::as_str)
                    == Some("gitattributes")
                {
                    return "gitattributes".into();
                }
            }
        }
        if object.get("gitignoreDestination").and_then(Value::as_str) == Some("gitattributes") {
            "gitattributes".into()
        } else {
            root.into()
        }
    }
    pub fn output_roots(&self, target: Option<&str>) -> Vec<PathBuf> {
        if self.global {
            if let Some(target) = target {
                let variable = match target {
                    "hermesagent" => Some("HERMES_HOME"),
                    "kimi-code" => Some("KIMI_CODE_HOME"),
                    _ => None,
                };
                if let Some(variable) = variable {
                    if let Some(value) =
                        std::env::var_os(variable).filter(|value| !value.is_empty())
                    {
                        let path = PathBuf::from(value);
                        return vec![if path.is_absolute() {
                            path
                        } else {
                            self.cwd.join(path)
                        }];
                    }
                }
            }
            return home_dir().map(|path| vec![path]).unwrap_or_default();
        }
        if let Some(target) = target {
            if !self.output_roots_by_target.is_empty() {
                return self
                    .output_roots_by_target
                    .get(target)
                    .cloned()
                    .unwrap_or_default();
            }
        }
        self.output_roots.clone().unwrap_or_default()
    }
    pub fn input_roots(&self) -> &[PathBuf] {
        &self.input_roots
    }
    pub fn delete(&self) -> bool {
        self.delete
    }
    pub fn global(&self) -> bool {
        self.global
    }
    pub fn verbose(&self) -> bool {
        self.verbose
    }
    pub fn silent(&self) -> bool {
        self.silent
    }
    pub fn simulate_commands(&self) -> bool {
        self.simulate_commands
    }
    pub fn simulate_subagents(&self) -> bool {
        self.simulate_subagents
    }
    pub fn simulate_skills(&self) -> bool {
        self.simulate_skills
    }
    pub fn dry_run(&self) -> bool {
        self.dry_run
    }
    pub fn check(&self) -> bool {
        self.check
    }
    pub fn gitignore_targets_only(&self) -> bool {
        self.gitignore_targets_only
    }
    pub fn preview(&self) -> bool {
        self.dry_run || self.check
    }
    pub fn flattened_command_naming(&self) -> &str {
        &self.flattened_command_naming
    }
}

fn absolute(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    let current = std::env::current_dir()?;
    Ok(current
        .join(path)
        .canonicalize()
        .unwrap_or_else(|_| current.join(path)))
}

fn resolve_config_path(
    cwd: &Path,
    config_path: Option<&str>,
    input_root: Option<&str>,
) -> Result<PathBuf> {
    let anchor = if let Some(input_root) = input_root {
        let root = PathBuf::from(input_root);
        if root.is_absolute() {
            root
        } else {
            cwd.join(root)
        }
    } else {
        cwd.to_path_buf()
    };
    let raw = config_path.unwrap_or("carabiner.jsonc");
    if raw.is_empty() || raw.contains('\0') {
        return Err(anyhow!("Path traversal detected in config path '{raw}'"));
    }
    if Path::new(raw)
        .components()
        .any(|component| component == std::path::Component::ParentDir)
    {
        return Err(anyhow!("Path traversal detected in config path '{raw}'"));
    }
    let candidate = PathBuf::from(raw);
    let resolved = if candidate.is_absolute() {
        candidate
    } else {
        anchor.join(candidate)
    };
    if !resolved.starts_with(&anchor) {
        return Err(anyhow!("Path traversal detected in config path '{raw}'"));
    }
    Ok(resolved)
}

fn load_config_file(path: &Path) -> Result<(Map<String, Value>, bool)> {
    if !path.exists() {
        return Ok((Map::new(), false));
    }
    let content =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let value =
        parse_jsonc(&content).with_context(|| format!("failed to parse {}", path.display()))?;
    let object = value
        .as_object()
        .ok_or_else(|| anyhow!("configuration {} must contain an object", path.display()))?;
    Ok((object.clone(), true))
}

fn validate_config_fields(config: &Map<String, Value>, path: &Path) -> Result<()> {
    let location = path.display().to_string();
    if config.contains_key("inputRoot") && config.contains_key("inputRoots") {
        return Err(anyhow!(
            "Invalid config: 'inputRoot' and 'inputRoots' cannot be combined (in {location})."
        ));
    }
    if let Some(value) = config.get("$schema") {
        if !value.is_string() {
            return Err(anyhow!(
                "Invalid '$schema' in {location}: expected a string."
            ));
        }
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
        if let Some(value) = config.get(key) {
            if !value.is_boolean() {
                return Err(anyhow!(
                    "Invalid '{key}' in {location}: expected a boolean."
                ));
            }
        }
    }
    for key in [
        "inputRoot",
        "flattenedCommandNaming",
        "gitignoreDestination",
    ] {
        if let Some(value) = config.get(key) {
            if !value.is_string() {
                return Err(anyhow!("Invalid '{key}' in {location}: expected a string."));
            }
        }
    }
    if let Some(value) = config.get("inputRoots") {
        let roots = value
            .as_array()
            .ok_or_else(|| anyhow!("Invalid 'inputRoots' in {location}: expected an array."))?;
        if roots.is_empty() {
            return Err(anyhow!(
                "Invalid config: 'inputRoots' must be non-empty (in {location})."
            ));
        }
        if roots.iter().any(|root| !root.is_string()) {
            return Err(anyhow!(
                "Invalid 'inputRoots' in {location}: entries must be strings."
            ));
        }
    }
    if let Some(value) = config.get("outputRoots") {
        match value {
            Value::Array(values) => {
                if values.is_empty() || values.iter().any(|value| !value.is_string()) {
                    return Err(anyhow!("Invalid 'outputRoots' in {location}: expected a non-empty array of strings."));
                }
            }
            Value::Object(entries) => {
                for (target, roots) in entries {
                    validate_target(target)?;
                    match roots {
                        Value::String(root) if !root.is_empty() => {}
                        Value::Array(values) if !values.is_empty() && values.iter().all(Value::is_string) => {}
                        _ => return Err(anyhow!("Invalid outputRoots entry '{target}' in {location}: expected a non-empty string or array of strings.")),
                    }
                }
            }
            _ => {
                return Err(anyhow!(
                    "Invalid 'outputRoots' in {location}: expected an array or object."
                ))
            }
        }
    }
    if let Some(root) = config.get("inputRoot").and_then(Value::as_str) {
        if root.is_empty() {
            return Err(anyhow!(
                "Invalid 'inputRoot' in {location}: it must not be empty."
            ));
        }
    }
    if let Some(value) = config.get("flattenedCommandNaming") {
        if !matches!(value.as_str(), Some("basename") | Some("path")) {
            return Err(anyhow!(
                "Invalid 'flattenedCommandNaming' in {location}: expected 'basename' or 'path'."
            ));
        }
    }
    if let Some(value) = config.get("gitignoreDestination") {
        if !matches!(value.as_str(), Some("gitignore") | Some("gitattributes")) {
            return Err(anyhow!("Invalid 'gitignoreDestination' in {location}: expected 'gitignore' or 'gitattributes'."));
        }
    }
    if let Some(value) = config.get("sources") {
        let sources = value
            .as_array()
            .ok_or_else(|| anyhow!("Invalid 'sources' in {location}: expected an array."))?;
        for (index, entry) in sources.iter().enumerate() {
            let object = entry.as_object().ok_or_else(|| {
                anyhow!("Invalid sources[{index}] in {location}: expected an object.")
            })?;
            let source = object
                .get("source")
                .and_then(Value::as_str)
                .filter(|source| !source.is_empty())
                .ok_or_else(|| anyhow!("Invalid sources[{index}] in {location}: source must be a non-empty string."))?;
            if source.chars().any(|character| character.is_control()) {
                return Err(anyhow!(
                    "Invalid sources[{index}] in {location}: source must not contain control characters."
                ));
            }
            for key in ["skills", "rules"] {
                if let Some(value) = object.get(key) {
                    let values = value.as_array().ok_or_else(|| {
                        anyhow!("Invalid sources[{index}].{key} in {location}: expected an array.")
                    })?;
                    if values.iter().any(|value| !value.is_string()) {
                        return Err(anyhow!(
                            "Invalid sources[{index}].{key} in {location}: entries must be strings."
                        ));
                    }
                }
            }
            if let Some(value) = object.get("transport") {
                let transport = value.as_str().ok_or_else(|| {
                    anyhow!("Invalid sources[{index}].transport in {location}: expected a string.")
                })?;
                if !matches!(transport, "github" | "git" | "npm") {
                    return Err(anyhow!(
                        "Invalid sources[{index}].transport '{transport}' in {location}."
                    ));
                }
            }
            for key in [
                "ref",
                "path",
                "rulesPath",
                "registry",
                "tokenEnv",
                "agent",
                "scope",
            ] {
                if let Some(value) = object.get(key) {
                    if !value.is_string() {
                        return Err(anyhow!(
                            "Invalid sources[{index}].{key} in {location}: expected a string."
                        ));
                    }
                }
            }
            if (object.contains_key("registry") || object.contains_key("tokenEnv"))
                && object.get("transport").and_then(Value::as_str) != Some("npm")
            {
                return Err(anyhow!(
                    "'registry' and 'tokenEnv' are only valid with transport 'npm' (sources[{index}] in {location})."
                ));
            }
            for key in ["ref", "path", "rulesPath"] {
                if let Some(value) = object.get(key).and_then(Value::as_str) {
                    if key == "ref"
                        && (value.starts_with('-') || value.chars().any(|c| c.is_control()))
                    {
                        return Err(anyhow!(
                            "Invalid sources[{index}].ref in {location}: ref must not start with '-' or contain control characters."
                        ));
                    }
                    if matches!(key, "path" | "rulesPath")
                        && (value.is_empty()
                            || Path::new(value).is_absolute()
                            || value.contains("..")
                            || value.chars().any(|c| c.is_control()))
                    {
                        return Err(anyhow!("Invalid sources[{index}].{key} in {location}: path must be relative and must not contain '..' or control characters."));
                    }
                }
            }
            if let Some(registry) = object.get("registry").and_then(Value::as_str) {
                if (!registry.starts_with("https://") && !registry.starts_with("http://"))
                    || registry.chars().any(|c| c.is_control())
                {
                    return Err(anyhow!("Invalid sources[{index}].registry in {location}: registry must be an http(s) URL without control characters."));
                }
            }
            if let Some(token_env) = object.get("tokenEnv").and_then(Value::as_str) {
                if token_env.is_empty()
                    || !token_env.chars().enumerate().all(|(position, character)| {
                        character == '_'
                            || character.is_ascii_alphanumeric()
                                && (position > 0 || character.is_ascii_alphabetic())
                    })
                {
                    return Err(anyhow!("Invalid sources[{index}].tokenEnv in {location}: invalid environment variable name."));
                }
            }
            if let Some(agent) = object.get("agent").and_then(Value::as_str) {
                if !matches!(
                    agent,
                    "github-copilot"
                        | "claude-code"
                        | "cursor"
                        | "codex"
                        | "gemini"
                        | "antigravity"
                ) {
                    return Err(anyhow!(
                        "Invalid sources[{index}].agent '{agent}' in {location}."
                    ));
                }
            }
            if let Some(scope) = object.get("scope").and_then(Value::as_str) {
                if !matches!(scope, "project" | "user") {
                    return Err(anyhow!(
                        "Invalid sources[{index}].scope '{scope}' in {location}."
                    ));
                }
            }
        }
    }
    Ok(())
}

fn merge_objects(mut base: Map<String, Value>, local: Map<String, Value>) -> Map<String, Value> {
    for (key, value) in local {
        base.insert(key, value);
    }
    base
}

fn validate_target(target: &str) -> Result<()> {
    if target == "*" {
        return Ok(());
    }
    if all_targets().iter().any(|known| known == target) {
        Ok(())
    } else {
        Err(anyhow!(
            "Invalid tool target '{target}'. Must be one of: {}",
            all_targets().join(", ")
        ))
    }
}

fn validate_targets(mut targets: Vec<String>) -> Result<Vec<String>> {
    if targets.iter().any(|target| target == "*") {
        let mut expanded = non_legacy_targets();
        for target in targets.drain(..) {
            if target != "*" && !expanded.contains(&target) {
                expanded.push(target);
            }
        }
        return Ok(unique(expanded));
    }
    for target in &targets {
        validate_target(target)?;
    }
    Ok(unique(targets))
}

fn parse_targets(value: Option<&Value>) -> Result<Vec<String>> {
    let Some(value) = value else {
        return Ok(vec!["agentsmd".into()]);
    };
    match value {
        Value::Array(items) => {
            let mut names = Vec::new();
            for item in items {
                let name = item
                    .as_str()
                    .ok_or_else(|| anyhow!("'targets' entries must be strings"))?;
                names.push(name.to_owned());
            }
            validate_targets(names)
        }
        Value::Object(object) => {
            for name in object.keys() {
                if name == "*" {
                    return Err(anyhow!(
                        "Invalid target '*' in object form: use the array form `targets: ['*']`."
                    ));
                }
                validate_target(name)?;
            }
            Ok(object.keys().cloned().collect())
        }
        _ => Err(anyhow!("'targets' must be an array or object")),
    }
}

fn parse_string_array(value: Option<&Value>) -> Result<Option<Vec<String>>> {
    let Some(value) = value else {
        return Ok(None);
    };
    let Some(items) = value.as_array() else {
        return Err(anyhow!("expected an array of strings"));
    };
    let mut values = Vec::new();
    for item in items {
        values.push(
            item.as_str()
                .ok_or_else(|| anyhow!("feature entries must be strings"))?
                .to_owned(),
        );
    }
    Ok(Some(values))
}

fn normalize_features(features: Vec<String>) -> Result<Vec<String>> {
    let mut result = Vec::new();
    let wildcard = features.iter().any(|feature| feature == "*");
    if wildcard {
        result.extend(all_features());
    }
    for feature in features {
        if feature == "*" {
            continue;
        }
        if !all_features().iter().any(|known| known == &feature) {
            return Err(anyhow!(
                "Invalid feature '{feature}'. Must be one of: {}",
                all_features().join(", ")
            ));
        }
        if !result.contains(&feature) {
            result.push(feature);
        }
    }
    Ok(result)
}

fn value_enabled(value: Option<&Value>) -> bool {
    matches!(value, Some(Value::Bool(true) | Value::Object(_)))
}

fn non_legacy_targets() -> Vec<String> {
    all_targets()
        .into_iter()
        .filter(|target| {
            !matches!(
                target.as_str(),
                "augmentcode-legacy"
                    | "claudecode-legacy"
                    | "antigravity-plugin"
                    | "claudecode-plugin"
            )
        })
        .collect()
}

fn unique(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    values
        .into_iter()
        .filter(|value| seen.insert(value.clone()))
        .collect()
}

fn validate_conflicting_targets(targets: &[String]) -> Result<()> {
    if targets.iter().any(|target| target == "augmentcode")
        && targets.iter().any(|target| target == "augmentcode-legacy")
    {
        return Err(anyhow!("Conflicting targets: 'augmentcode' and 'augmentcode-legacy' cannot be used together. Please choose one."));
    }
    if targets.iter().any(|target| target == "claudecode")
        && targets.iter().any(|target| target == "claudecode-legacy")
    {
        return Err(anyhow!("Conflicting targets: 'claudecode' and 'claudecode-legacy' cannot be used together. Please choose one."));
    }
    Ok(())
}

fn resolve_input_roots(
    cwd: &Path,
    options: &ConfigOptions,
    config: &Map<String, Value>,
) -> Result<Vec<PathBuf>> {
    let values: Vec<String> = if let Some(roots) = &options.input_roots {
        if roots.is_empty() {
            return Err(anyhow!("Invalid config: 'inputRoots' must be non-empty."));
        }
        roots.clone()
    } else if let Some(root) = &options.input_root {
        vec![PathBuf::from(root)
            .join(".carabiner")
            .to_string_lossy()
            .into_owned()]
    } else if let Some(Value::Array(roots)) = config.get("inputRoots") {
        if roots.is_empty() {
            return Err(anyhow!("Invalid config: 'inputRoots' must be non-empty."));
        }
        roots
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .ok_or_else(|| anyhow!("'inputRoots' entries must be strings"))
                    .map(ToOwned::to_owned)
            })
            .collect::<Result<Vec<_>>>()?
    } else if let Some(root) = config.get("inputRoot").and_then(Value::as_str) {
        vec![PathBuf::from(root)
            .join(".carabiner")
            .to_string_lossy()
            .into_owned()]
    } else {
        vec![".carabiner".into()]
    };
    let mut resolved = Vec::new();
    for value in values {
        if value.trim().is_empty() || value.chars().any(|character| character.is_control()) {
            return Err(anyhow!(
                "input root must be a non-empty path without control characters"
            ));
        }
        if value.split(&['/', '\\'][..]).any(|segment| segment == "..") {
            return Err(anyhow!("path traversal detected in input root '{value}'"));
        }
        let path = PathBuf::from(&value);
        let absolute = if path.is_absolute() {
            let normalized = lexical_normalize(&path);
            if normalized != path {
                return Err(anyhow!(
                    "input root must be a normalized absolute path: {value}"
                ));
            }
            path
        } else {
            cwd.join(path)
        };
        if !resolved.contains(&absolute) {
            resolved.push(absolute);
        }
    }
    Ok(resolved)
}

fn resolve_output_roots(cwd: &Path, value: &Value, global: bool) -> Result<ResolvedOutputRoots> {
    if global {
        return Ok((None, HashMap::new()));
    }
    if let Value::Array(values) = value {
        if values.is_empty() {
            return Err(anyhow!("'outputRoots' must be non-empty."));
        }
        let roots = values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .ok_or_else(|| anyhow!("'outputRoots' entries must be strings"))
            })
            .map(|root| root.and_then(|root| resolve_output_root(cwd, root)))
            .collect::<Result<Vec<_>>>()?;
        return Ok((Some(roots), HashMap::new()));
    }
    let Some(object) = value.as_object() else {
        return Err(anyhow!("'outputRoots' must be an array or object"));
    };
    let known = all_targets();
    let mut result = HashMap::new();
    for (target, roots_value) in object {
        if !known.iter().any(|known| known == target) {
            return Err(anyhow!("Unknown outputRoots target '{target}'"));
        }
        let roots = match roots_value {
            Value::String(root) => vec![resolve_output_root(cwd, root)?],
            Value::Array(values) => {
                if values.is_empty() {
                    return Err(anyhow!("outputRoots entry '{target}' must be non-empty."));
                }
                values
                    .iter()
                    .map(|value| {
                        value.as_str().ok_or_else(|| {
                            anyhow!("outputRoots entry '{target}' must contain strings")
                        })
                    })
                    .map(|root| root.and_then(|root| resolve_output_root(cwd, root)))
                    .collect::<Result<Vec<_>>>()?
            }
            _ => {
                return Err(anyhow!(
                    "outputRoots entry '{target}' must be a string or array"
                ))
            }
        };
        result.insert(target.clone(), roots);
    }
    Ok((None, result))
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            std::path::Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            std::path::Component::RootDir | std::path::Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
}

fn resolve_output_root(cwd: &Path, value: &str) -> Result<PathBuf> {
    if value.trim().is_empty() {
        return Err(anyhow!("outputRoot cannot be an empty string"));
    }
    if value.chars().any(|character| character.is_control())
        || value.split(&['/', '\\'][..]).any(|segment| segment == "..")
    {
        return Err(anyhow!("invalid output root '{value}'"));
    }
    let path = PathBuf::from(value);
    let resolved = if path.is_absolute() {
        let normalized = lexical_normalize(&path);
        if normalized != path {
            return Err(anyhow!(
                "outputRoot must be a normalized absolute path: {value}"
            ));
        }
        path
    } else {
        cwd.join(path)
    };
    if resolved.parent() == Some(resolved.as_path()) {
        return Err(anyhow!(
            "outputRoot must not be the filesystem root: {value}"
        ));
    }
    Ok(resolved)
}

/// A stable data shape used by callers that want to report feature counts.
pub fn empty_feature_results() -> HashMap<String, FeatureResult> {
    Feature::ALL
        .iter()
        .map(|feature| (feature.to_string(), FeatureResult::default()))
        .collect()
}

pub fn version() -> &'static str {
    VERSION
}
