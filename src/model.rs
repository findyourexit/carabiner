use serde_json::{Map, Value};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Feature {
    Rules,
    Ignore,
    Mcp,
    Commands,
    Subagents,
    Skills,
    Hooks,
    Permissions,
    Checks,
}
pub const ALL_FEATURES: [&str; 9] = [
    "rules",
    "ignore",
    "mcp",
    "subagents",
    "commands",
    "skills",
    "hooks",
    "permissions",
    "checks",
];

impl Feature {
    pub const ALL: [Feature; 9] = [
        Self::Rules,
        Self::Ignore,
        Self::Mcp,
        Self::Subagents,
        Self::Commands,
        Self::Skills,
        Self::Hooks,
        Self::Permissions,
        Self::Checks,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rules => "rules",
            Self::Ignore => "ignore",
            Self::Mcp => "mcp",
            Self::Commands => "commands",
            Self::Subagents => "subagents",
            Self::Skills => "skills",
            Self::Hooks => "hooks",
            Self::Permissions => "permissions",
            Self::Checks => "checks",
        }
    }
}

impl std::fmt::Display for Feature {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub relative_path: String,
    pub frontmatter: Map<String, Value>,
    pub body: String,
}

impl Rule {
    pub fn root(&self) -> bool {
        self.frontmatter
            .get("root")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    pub fn local_root(&self) -> bool {
        self.frontmatter
            .get("localRoot")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    pub fn targeted_at(&self, target: &str) -> bool {
        match self.frontmatter.get("targets") {
            None => true,
            Some(Value::Array(targets)) => targets.iter().any(|value| {
                value
                    .as_str()
                    .map(|name| name == "*" || name == target)
                    .unwrap_or(false)
            }),
            _ => true,
        }
    }

    pub fn description(&self) -> Option<&str> {
        self.frontmatter.get("description").and_then(Value::as_str)
    }

    pub fn globs(&self) -> Vec<String> {
        self.frontmatter
            .get("globs")
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(ToOwned::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone)]
pub struct Command {
    pub relative_path: String,
    pub frontmatter: Map<String, Value>,
    pub body: String,
}

impl Command {
    pub fn targeted_at(&self, target: &str) -> bool {
        match self.frontmatter.get("targets") {
            None => true,
            Some(Value::Array(targets)) => targets.iter().any(|value| {
                value
                    .as_str()
                    .map(|name| name == "*" || name == target)
                    .unwrap_or(false)
            }),
            _ => true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Subagent {
    pub relative_path: String,
    pub frontmatter: Map<String, Value>,
    pub body: String,
}

impl Subagent {
    pub fn name(&self) -> String {
        self.frontmatter
            .get("name")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| {
                std::path::Path::new(&self.relative_path)
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .unwrap_or("agent")
                    .to_owned()
            })
    }

    pub fn targeted_at(&self, target: &str) -> bool {
        match self.frontmatter.get("targets") {
            None => true,
            Some(Value::Array(targets)) => targets.iter().any(|value| {
                value
                    .as_str()
                    .map(|name| name == "*" || name == target)
                    .unwrap_or(false)
            }),
            _ => true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SkillFile {
    pub relative_path: String,
    pub content: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub frontmatter: Map<String, Value>,
    pub body: String,
    pub other_files: Vec<SkillFile>,
}

impl Skill {
    pub fn targeted_at(&self, target: &str) -> bool {
        match self.frontmatter.get("targets") {
            None => true,
            Some(Value::Array(targets)) => targets.iter().any(|value| {
                value
                    .as_str()
                    .map(|name| name == "*" || name == target)
                    .unwrap_or(false)
            }),
            _ => true,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CanonicalModel {
    pub rules: Vec<Rule>,
    pub commands: Vec<Command>,
    pub subagents: Vec<Subagent>,
    pub skills: Vec<Skill>,
    pub mcp: Option<Value>,
    pub hooks: Option<Value>,
    pub permissions: Option<Value>,
    pub ignore: Option<String>,
    pub checks: Vec<Command>,
}

impl CanonicalModel {
    pub fn has_feature(&self, feature: Feature) -> bool {
        match feature {
            Feature::Rules => !self.rules.is_empty(),
            Feature::Ignore => self.ignore.is_some(),
            Feature::Mcp => self.mcp.is_some(),
            Feature::Commands => !self.commands.is_empty(),
            Feature::Subagents => !self.subagents.is_empty(),
            Feature::Skills => !self.skills.is_empty(),
            Feature::Hooks => self.hooks.is_some(),
            Feature::Permissions => self.permissions.is_some(),
            Feature::Checks => !self.checks.is_empty(),
        }
    }
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct FeatureResult {
    pub count: usize,
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct GenerateResult {
    pub rules: FeatureResult,
    pub ignore: FeatureResult,
    pub mcp: FeatureResult,
    pub commands: FeatureResult,
    pub subagents: FeatureResult,
    pub skills: FeatureResult,
    pub hooks: FeatureResult,
    pub permissions: FeatureResult,
    pub checks: FeatureResult,
    pub activation: FeatureResult,
    /// Serialized skill descriptors retained for the CLI JSON contract.
    pub skill_details: Vec<Value>,
    pub has_diff: bool,
}

impl GenerateResult {
    pub fn total_files(&self) -> usize {
        [
            &self.rules,
            &self.ignore,
            &self.mcp,
            &self.commands,
            &self.subagents,
            &self.skills,
            &self.hooks,
            &self.permissions,
            &self.checks,
            &self.activation,
        ]
        .iter()
        .map(|result| result.count)
        .sum()
    }

    pub fn feature(&self, feature: Feature) -> &FeatureResult {
        match feature {
            Feature::Rules => &self.rules,
            Feature::Ignore => &self.ignore,
            Feature::Mcp => &self.mcp,
            Feature::Commands => &self.commands,
            Feature::Subagents => &self.subagents,
            Feature::Skills => &self.skills,
            Feature::Hooks => &self.hooks,
            Feature::Permissions => &self.permissions,
            Feature::Checks => &self.checks,
        }
    }
    pub fn flat(&self) -> FlatGenerateResult {
        self.into()
    }
}
#[derive(Debug, Clone, serde::Serialize)]
pub struct FlatGenerateResult {
    #[serde(rename = "rulesCount")]
    pub rules_count: usize,
    #[serde(rename = "rulesPaths")]
    pub rules_paths: Vec<String>,
    #[serde(rename = "ignoreCount")]
    pub ignore_count: usize,
    #[serde(rename = "ignorePaths")]
    pub ignore_paths: Vec<String>,
    #[serde(rename = "mcpCount")]
    pub mcp_count: usize,
    #[serde(rename = "mcpPaths")]
    pub mcp_paths: Vec<String>,
    #[serde(rename = "commandsCount")]
    pub commands_count: usize,
    #[serde(rename = "commandsPaths")]
    pub commands_paths: Vec<String>,
    #[serde(rename = "subagentsCount")]
    pub subagents_count: usize,
    #[serde(rename = "subagentsPaths")]
    pub subagents_paths: Vec<String>,
    #[serde(rename = "skillsCount")]
    pub skills_count: usize,
    #[serde(rename = "skillsPaths")]
    pub skills_paths: Vec<String>,
    #[serde(rename = "hooksCount")]
    pub hooks_count: usize,
    #[serde(rename = "hooksPaths")]
    pub hooks_paths: Vec<String>,
    #[serde(rename = "permissionsCount")]
    pub permissions_count: usize,
    #[serde(rename = "permissionsPaths")]
    pub permissions_paths: Vec<String>,
    #[serde(rename = "checksCount")]
    pub checks_count: usize,
    #[serde(rename = "checksPaths")]
    pub checks_paths: Vec<String>,
    #[serde(rename = "activationCount")]
    pub activation_count: usize,
    #[serde(rename = "activationPaths")]
    pub activation_paths: Vec<String>,
    pub skills: Vec<Value>,
    #[serde(rename = "hasDiff")]
    pub has_diff: bool,
}

impl From<&GenerateResult> for FlatGenerateResult {
    fn from(result: &GenerateResult) -> Self {
        Self {
            rules_count: result.rules.count,
            rules_paths: result.rules.paths.clone(),
            ignore_count: result.ignore.count,
            ignore_paths: result.ignore.paths.clone(),
            mcp_count: result.mcp.count,
            mcp_paths: result.mcp.paths.clone(),
            commands_count: result.commands.count,
            commands_paths: result.commands.paths.clone(),
            subagents_count: result.subagents.count,
            subagents_paths: result.subagents.paths.clone(),
            skills_count: result.skills.count,
            skills_paths: result.skills.paths.clone(),
            hooks_count: result.hooks.count,
            hooks_paths: result.hooks.paths.clone(),
            permissions_count: result.permissions.count,
            permissions_paths: result.permissions.paths.clone(),
            checks_count: result.checks.count,
            checks_paths: result.checks.paths.clone(),
            activation_count: result.activation.count,
            activation_paths: result.activation.paths.clone(),
            skills: result.skill_details.clone(),
            has_diff: result.has_diff,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FlatImportResult {
    #[serde(rename = "rulesCount")]
    pub rules_count: usize,
    #[serde(rename = "ignoreCount")]
    pub ignore_count: usize,
    #[serde(rename = "mcpCount")]
    pub mcp_count: usize,
    #[serde(rename = "commandsCount")]
    pub commands_count: usize,
    #[serde(rename = "subagentsCount")]
    pub subagents_count: usize,
    #[serde(rename = "skillsCount")]
    pub skills_count: usize,
    #[serde(rename = "hooksCount")]
    pub hooks_count: usize,
    #[serde(rename = "permissionsCount")]
    pub permissions_count: usize,
    #[serde(rename = "checksCount")]
    pub checks_count: usize,
}

impl From<&ImportResult> for FlatImportResult {
    fn from(result: &ImportResult) -> Self {
        Self {
            rules_count: result.rules,
            ignore_count: result.ignore,
            mcp_count: result.mcp,
            commands_count: result.commands,
            subagents_count: result.subagents,
            skills_count: result.skills,
            hooks_count: result.hooks,
            permissions_count: result.permissions,
            checks_count: result.checks,
        }
    }
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ImportResult {
    pub rules: usize,
    pub ignore: usize,
    pub mcp: usize,
    pub commands: usize,
    pub subagents: usize,
    pub skills: usize,
    pub hooks: usize,
    pub permissions: usize,
    pub checks: usize,
}

impl ImportResult {
    pub fn total_files(&self) -> usize {
        self.rules
            + self.ignore
            + self.mcp
            + self.commands
            + self.subagents
            + self.skills
            + self.hooks
            + self.permissions
            + self.checks
    }
    pub fn flat(&self) -> FlatImportResult {
        self.into()
    }
}
pub type ConvertResult = ImportResult;

#[derive(Debug, Clone)]
pub struct GeneratedFile {
    pub relative_path: String,
    pub content: Vec<u8>,
    pub feature: Feature,
    pub binary: bool,
}

impl GeneratedFile {
    pub fn text(relative_path: impl Into<String>, content: String, feature: Feature) -> Self {
        Self {
            relative_path: relative_path.into(),
            content: content.into_bytes(),
            feature,
            binary: false,
        }
    }

    pub fn binary(relative_path: impl Into<String>, content: Vec<u8>, feature: Feature) -> Self {
        Self {
            relative_path: relative_path.into(),
            content,
            feature,
            binary: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LoadedFile {
    pub path: PathBuf,
    pub content: String,
}
