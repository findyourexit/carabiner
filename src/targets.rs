use crate::model::{Feature, ALL_FEATURES};
use std::collections::HashSet;

pub const VERSION: &str = "0.1.1";
pub const ALL_TOOL_TARGETS: [&str; 42] = [
    "agentsmd",
    "aiassistant",
    "amp",
    "antigravity-cli",
    "antigravity-ide",
    "antigravity-plugin",
    "augmentcode",
    "augmentcode-legacy",
    "claudecode",
    "claudecode-legacy",
    "cline",
    "codexcli",
    "copilot",
    "copilotcli",
    "cursor",
    "deepagents",
    "factorydroid",
    "goose",
    "grokcli",
    "hermesagent",
    "junie",
    "kilo",
    "kimi-code",
    "kiro",
    "kiro-cli",
    "kiro-ide",
    "musecode",
    "opencode",
    "pi",
    "qwencode",
    "reasonix",
    "replit",
    "roo",
    "rovodev",
    "takt",
    "vibe",
    "warp",
    "devin",
    "zed",
    "zoocode",
    "claudecode-plugin",
    "agentsskills",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RuleMode {
    #[default]
    Modular,
    Fold,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DataFormat {
    #[default]
    JsonMcpServers,
    JsonServers,
    JsonOpenCode,
    JsonAmp,
    JsonGeneric,
    TomlCodex,
    TomlReasonix,
    TomlVibe,
    TomlGrok,
    YamlGoose,
    YamlHermes,
    YamlTakt,
}

#[derive(Debug, Clone)]
pub struct PathSpec {
    pub dir: String,
    pub file: String,
}

impl PathSpec {
    pub fn new(dir: impl Into<String>, file: impl Into<String>) -> Self {
        Self {
            dir: dir.into(),
            file: file.into(),
        }
    }

    pub fn path(&self) -> String {
        if self.dir.is_empty() || self.dir == "." {
            self.file.clone()
        } else {
            format!("{}/{}", self.dir.trim_end_matches('/'), self.file)
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ScopePaths {
    pub root_rule: Option<PathSpec>,
    pub nonroot_rule_dir: Option<String>,
    pub rule_ext: String,
    pub rule_mode: RuleMode,
    pub local_rule: Option<PathSpec>,
    pub command_dir: Option<String>,
    pub command_ext: String,
    pub subagent_dir: Option<String>,
    pub subagent_ext: String,
    pub aggregate_subagents: bool,
    pub skill_dir: Option<String>,
    pub mcp: Option<PathSpec>,
    pub mcp_format: DataFormat,
    pub hooks: Option<PathSpec>,
    pub permissions: Option<PathSpec>,
    pub ignore: Option<PathSpec>,
    pub checks: Option<PathSpec>,
}

impl ScopePaths {
    fn empty() -> Self {
        Self {
            rule_ext: "md".into(),
            rule_mode: RuleMode::Modular,
            command_ext: "md".into(),
            subagent_ext: "md".into(),
            mcp_format: DataFormat::JsonMcpServers,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone)]
pub struct TargetSpec {
    pub name: String,
    pub project: ScopePaths,
    pub global: ScopePaths,
    pub project_features: HashSet<Feature>,
    pub global_features: HashSet<Feature>,
    pub simulated: HashSet<Feature>,
}

pub fn all_features() -> Vec<String> {
    ALL_FEATURES
        .iter()
        .map(|feature| (*feature).to_owned())
        .collect()
}

pub fn all_targets() -> Vec<String> {
    ALL_TOOL_TARGETS
        .iter()
        .map(|value| (*value).to_owned())
        .collect()
}

fn set_features(names: &[&str]) -> HashSet<Feature> {
    names
        .iter()
        .filter_map(|name| match *name {
            "rules" => Some(Feature::Rules),
            "ignore" => Some(Feature::Ignore),
            "mcp" => Some(Feature::Mcp),
            "commands" => Some(Feature::Commands),
            "subagents" => Some(Feature::Subagents),
            "skills" => Some(Feature::Skills),
            "hooks" => Some(Feature::Hooks),
            "permissions" => Some(Feature::Permissions),
            "checks" => Some(Feature::Checks),
            _ => None,
        })
        .collect()
}

fn path(dir: &str, file: &str) -> PathSpec {
    PathSpec::new(dir, file)
}

fn strip_profile_prefix(value: &str, prefix: &str) -> String {
    if value == prefix {
        ".".into()
    } else if let Some(rest) = value.strip_prefix(&format!("{prefix}/")) {
        rest.into()
    } else {
        value.into()
    }
}

fn strip_profile_prefix_from_paths(paths: &mut ScopePaths, prefix: &str) {
    if let Some(path) = paths.root_rule.as_mut() {
        path.dir = strip_profile_prefix(&path.dir, prefix);
    }
    if let Some(path) = paths.nonroot_rule_dir.as_mut() {
        *path = strip_profile_prefix(path, prefix);
    }
    if let Some(path) = paths.local_rule.as_mut() {
        path.dir = strip_profile_prefix(&path.dir, prefix);
    }
    if let Some(path) = paths.command_dir.as_mut() {
        *path = strip_profile_prefix(path, prefix);
    }
    if let Some(path) = paths.subagent_dir.as_mut() {
        *path = strip_profile_prefix(path, prefix);
    }
    if let Some(path) = paths.skill_dir.as_mut() {
        *path = strip_profile_prefix(path, prefix);
    }
    for path in [
        &mut paths.mcp,
        &mut paths.hooks,
        &mut paths.permissions,
        &mut paths.ignore,
        &mut paths.checks,
    ]
    .into_iter()
    .flatten()
    {
        path.dir = strip_profile_prefix(&path.dir, prefix);
    }
}

fn project_and_global(name: &str) -> (ScopePaths, ScopePaths) {
    let mut project = ScopePaths::empty();
    let mut global = ScopePaths::empty();

    match name {
        "agentsmd" => {
            project.root_rule = Some(path(".", "AGENTS.md"));
            project.nonroot_rule_dir = Some(".agents/memories".into());
            project.command_dir = Some(".agents/commands".into());
            project.subagent_dir = Some(".agents/agents".into());
            project.skill_dir = Some(".agents/skills".into());
            project.rule_mode = RuleMode::Modular;
        }
        "agentsskills" => {
            project.skill_dir = Some(".agents/skills".into());
            global.skill_dir = Some(".agents/skills".into());
        }
        "aiassistant" => {
            project.nonroot_rule_dir = Some(".aiassistant/rules".into());
            project.skill_dir = Some(".agents/skills".into());
            project.mcp = Some(path(".ai/mcp", "mcp.json"));
            project.ignore = Some(path(".", ".aiignore"));
            global.mcp = Some(path(".ai/mcp", "mcp.json"));
        }
        "amp" => {
            project.root_rule = Some(path(".", "AGENTS.md"));
            project.nonroot_rule_dir = Some(".agents/memories".into());
            project.skill_dir = Some(".agents/skills".into());
            project.mcp = Some(path(".amp", "settings.json"));
            project.hooks = Some(path(".amp/plugins", "carabiner-hooks.ts"));
            project.permissions = Some(path(".amp", "settings.json"));
            project.checks = Some(path(".agents/checks", "__dynamic__"));
            global.root_rule = Some(path(".config/amp", "AGENTS.md"));
            global.nonroot_rule_dir = None;
            global.skill_dir = Some(".config/agents/skills".into());
            global.mcp = Some(path(".config/amp", "settings.json"));
            global.hooks = Some(path(".config/amp/plugins", "carabiner-hooks.ts"));
            global.permissions = Some(path(".config/amp", "settings.json"));
            global.checks = Some(path(".config/amp/checks", "__dynamic__"));
        }
        "antigravity-cli" => {
            project.root_rule = Some(path(".", "AGENTS.md"));
            project.nonroot_rule_dir = Some(".agents/rules".into());
            project.command_dir = Some(".agents/workflows".into());
            project.subagent_dir = Some(".agents/agents".into());
            project.skill_dir = Some(".agents/skills".into());
            project.mcp = Some(path(".agents", "mcp_config.json"));
            project.hooks = Some(path(".agents", "hooks.json"));
            project.ignore = Some(path(".", ".geminiignore"));
            global.root_rule = Some(path(".gemini", "GEMINI.md"));
            global.nonroot_rule_dir = None;
            global.skill_dir = Some(".gemini/antigravity-cli/skills".into());
            global.command_dir = Some(".gemini/antigravity-cli/global_workflows".into());
            global.subagent_dir = Some(".gemini/config/agents".into());
            global.hooks = Some(path(".gemini/config", "hooks.json"));
            global.mcp = Some(path(".gemini/config", "mcp_config.json"));
            global.permissions = Some(path(".gemini/antigravity-cli", "settings.json"));
        }
        "antigravity-ide" => {
            project.root_rule = Some(path(".", "AGENTS.md"));
            project.nonroot_rule_dir = Some(".agents/rules".into());
            project.command_dir = Some(".agents/workflows".into());
            project.subagent_dir = Some(".agents/agents".into());
            project.skill_dir = Some(".agents/skills".into());
            project.mcp = Some(path(".agents", "mcp_config.json"));
            project.hooks = Some(path(".agents", "hooks.json"));
            project.permissions = Some(path(".antigravity", "settings.json"));
            global.root_rule = Some(path(".gemini", "GEMINI.md"));
            global.nonroot_rule_dir = None;
            global.command_dir = Some(".gemini/antigravity/global_workflows".into());
            global.subagent_dir = Some(".gemini/config/agents".into());
            global.skill_dir = Some(".gemini/config/skills".into());
            global.mcp = Some(path(".gemini/config", "mcp_config.json"));
            global.hooks = Some(path(".gemini/config", "hooks.json"));
        }
        "antigravity-plugin" => {
            project.root_rule = Some(path("rules", "AGENTS.md"));
            project.nonroot_rule_dir = Some("rules".into());
            project.skill_dir = Some("skills".into());
            project.subagent_dir = Some("agents".into());
            project.mcp = Some(path(".", "mcp_config.json"));
            project.hooks = Some(path(".", "hooks.json"));
        }
        "augmentcode" => {
            project.root_rule = None;
            project.nonroot_rule_dir = Some(".augment/rules".into());
            project.command_dir = Some(".augment/commands".into());
            project.subagent_dir = Some(".augment/agents".into());
            project.skill_dir = Some(".augment/skills".into());
            project.mcp = Some(path(".augment", "settings.json"));
            project.hooks = Some(path(".augment", "settings.json"));
            project.permissions = Some(path(".augment", "settings.json"));
            project.ignore = Some(path(".", ".augmentignore"));
            project.checks = Some(path(".augment", "code_review_guidelines.yaml"));
            global.root_rule = None;
            global.nonroot_rule_dir = Some(".augment/rules".into());
            global.command_dir = Some(".augment/commands".into());
            global.subagent_dir = Some(".augment/agents".into());
            global.skill_dir = Some(".augment/skills".into());
            global.mcp = Some(path(".augment", "settings.json"));
            global.hooks = Some(path(".augment", "settings.json"));
            global.permissions = Some(path(".augment", "settings.json"));
        }
        "claudecode-plugin" => {
            project.command_dir = Some("commands".into());
            project.subagent_dir = Some("agents".into());
            project.skill_dir = Some("skills".into());
            project.mcp = Some(path(".", ".mcp.json"));
            project.hooks = Some(path("hooks", "hooks.json"));
        }
        "augmentcode-legacy" => {
            project.root_rule = Some(path(".", ".augment-guidelines"));
            project.nonroot_rule_dir = Some(".augment/rules".into());
        }
        "claudecode" => {
            project.root_rule = Some(path(".", "CLAUDE.md"));
            project.nonroot_rule_dir = Some(".claude/rules".into());
            project.local_rule = Some(path(".", "CLAUDE.local.md"));
            project.command_dir = Some(".claude/commands".into());
            project.subagent_dir = Some(".claude/agents".into());
            project.skill_dir = Some(".claude/skills".into());
            project.mcp = Some(path(".", ".mcp.json"));
            project.hooks = Some(path(".claude", "settings.json"));
            project.permissions = Some(path(".claude", "settings.json"));
            project.ignore = Some(path(".claude", "settings.json"));
            global.root_rule = Some(path(".claude", "CLAUDE.md"));
            global.nonroot_rule_dir = Some(".claude/rules".into());
            global.command_dir = Some(".claude/commands".into());
            global.subagent_dir = Some(".claude/agents".into());
            global.skill_dir = Some(".claude/skills".into());
            global.mcp = Some(path(".", ".claude.json"));
            global.hooks = Some(path(".claude", "settings.json"));
            global.permissions = Some(path(".claude", "settings.json"));
        }
        "claudecode-legacy" => {
            project.root_rule = Some(path(".", "CLAUDE.md"));
            project.nonroot_rule_dir = Some(".claude/memories".into());
            project.local_rule = Some(path(".", "CLAUDE.local.md"));
            project.command_dir = Some(".claude/commands".into());
            project.subagent_dir = Some(".claude/agents".into());
            project.skill_dir = Some(".claude/skills".into());
            project.mcp = Some(path(".", ".mcp.json"));
            project.hooks = Some(path(".claude", "settings.json"));
            project.permissions = Some(path(".claude", "settings.json"));
            project.ignore = Some(path(".claude", "settings.json"));
            global.root_rule = Some(path(".claude", "CLAUDE.md"));
            global.command_dir = Some(".claude/commands".into());
            global.subagent_dir = Some(".claude/agents".into());
            global.skill_dir = Some(".claude/skills".into());
            global.mcp = Some(path(".", ".claude.json"));
            global.hooks = Some(path(".claude", "settings.json"));
            global.permissions = Some(path(".claude", "settings.json"));
        }
        "cline" => {
            project.root_rule = Some(path(".", "AGENTS.md"));
            project.nonroot_rule_dir = Some(".clinerules".into());
            project.command_dir = Some(".clinerules/workflows".into());
            project.subagent_dir = Some(".cline/agents".into());
            project.subagent_ext = "yaml".into();
            project.skill_dir = Some(".cline/skills".into());
            project.hooks = Some(path(".clinerules/hooks", "carabiner-hooks.json"));
            project.permissions = Some(path(".cline", "command-permissions.json"));
            project.ignore = Some(path(".", ".clineignore"));
            global.root_rule = Some(path(".agents", "AGENTS.md"));
            global.nonroot_rule_dir = Some("Documents/Cline/Rules".into());
            global.command_dir = Some("Documents/Cline/Workflows".into());
            global.subagent_dir = Some(".cline/agents".into());
            global.subagent_ext = "yaml".into();
            global.skill_dir = Some(".cline/skills".into());
            global.mcp = Some(path(".cline/data/settings", "cline_mcp_settings.json"));
            global.hooks = Some(path("Documents/Cline/Hooks", "carabiner-hooks.json"));
            global.permissions = Some(path(".cline", "command-permissions.json"));
        }
        "codexcli" => {
            project.root_rule = Some(path(".", "AGENTS.md"));
            project.skill_dir = Some(".agents/skills".into());
            project.subagent_dir = Some(".codex/agents".into());
            project.subagent_ext = "toml".into();
            project.mcp = Some(path(".codex", "config.toml"));
            project.hooks = Some(path(".codex", "hooks.json"));
            project.permissions = Some(path(".codex", "config.toml"));
            global.root_rule = Some(path(".codex", "AGENTS.md"));
            global.command_dir = Some(".codex/prompts".into());
            global.subagent_ext = "toml".into();
            global.subagent_dir = Some(".codex/agents".into());
            global.skill_dir = Some(".agents/skills".into());
            global.mcp = Some(path(".codex", "config.toml"));
            global.hooks = Some(path(".codex", "hooks.json"));
            global.permissions = Some(path(".codex", "config.toml"));
        }
        "copilot" => {
            project.root_rule = Some(path(".github", "copilot-instructions.md"));
            project.nonroot_rule_dir = Some(".github/instructions".into());
            project.rule_ext = ".instructions.md".into();
            project.command_dir = Some(".github/prompts".into());
            project.command_ext = "prompt.md".into();
            project.subagent_dir = Some(".github/agents".into());
            project.subagent_ext = "agent.md".into();
            project.skill_dir = Some(".github/skills".into());
            project.mcp = Some(path(".vscode", "mcp.json"));
            project.hooks = Some(path(".github/hooks", "copilot-hooks.json"));
            project.permissions = Some(path(".vscode", "settings.json"));
            global.root_rule = Some(path(".copilot", "copilot-instructions.md"));
            global.nonroot_rule_dir = Some(".copilot/instructions".into());
            global.rule_ext = ".instructions.md".into();
            global.subagent_dir = Some(".copilot/agents".into());
            global.subagent_ext = "agent.md".into();
            global.skill_dir = Some(".copilot/skills".into());
            global.hooks = Some(path(".copilot/hooks", "copilot-ide-hooks.json"));
        }
        "copilotcli" => {
            project.root_rule = Some(path(".github", "copilot-instructions.md"));
            project.nonroot_rule_dir = Some(".github/instructions".into());
            project.rule_ext = ".instructions.md".into();
            project.subagent_dir = Some(".github/agents".into());
            project.subagent_ext = "agent.md".into();
            project.skill_dir = Some(".github/skills".into());
            project.mcp = Some(path(".github", "mcp.json"));
            project.hooks = Some(path(".github/hooks", "copilotcli-hooks.json"));
            project.permissions = Some(path(".github/copilot", "settings.json"));
            global.root_rule = Some(path(".copilot", "copilot-instructions.md"));
            global.nonroot_rule_dir = Some(".copilot/instructions".into());
            global.rule_ext = ".instructions.md".into();
            global.subagent_dir = Some(".copilot/agents".into());
            global.subagent_ext = "agent.md".into();
            global.skill_dir = Some(".copilot/skills".into());
            global.hooks = Some(path(".copilot/hooks", "copilot-hooks.json"));
            global.mcp = Some(path(".copilot", "mcp-config.json"));
            global.permissions = Some(path(".copilot", "settings.json"));
        }
        "cursor" => {
            project.nonroot_rule_dir = Some(".cursor/rules".into());
            project.rule_ext = "mdc".into();
            project.command_dir = Some(".cursor/commands".into());
            project.subagent_dir = Some(".cursor/agents".into());
            project.skill_dir = Some(".cursor/skills".into());
            project.mcp = Some(path(".cursor", "mcp.json"));
            project.hooks = Some(path(".cursor", "hooks.json"));
            project.permissions = Some(path(".cursor", "cli.json"));
            project.ignore = Some(path(".", ".cursorignore"));
            project.checks = Some(path(".cursor", "BUGBOT.md"));
            global.nonroot_rule_dir = Some(".cursor/rules".into());
            global.command_dir = Some(".cursor/commands".into());
            global.subagent_dir = Some(".cursor/agents".into());
            global.skill_dir = Some(".cursor/skills".into());
            global.mcp = Some(path(".cursor", "mcp.json"));
            global.hooks = Some(path(".cursor", "hooks.json"));
            global.permissions = Some(path(".cursor", "cli-config.json"));
        }
        "deepagents" => {
            project.root_rule = Some(path(".deepagents", "AGENTS.md"));
            project.rule_mode = RuleMode::Fold;
            project.skill_dir = Some(".deepagents/skills".into());
            project.subagent_dir = Some(".deepagents/agents".into());
            project.subagent_ext = "AGENTS.md".into();
            project.mcp = Some(path(".deepagents", ".mcp.json"));
            project.hooks = Some(path(".deepagents", "hooks.json"));
            global.root_rule = Some(path(".deepagents/deepagents", "AGENTS.md"));
            global.rule_mode = RuleMode::Fold;
            global.skill_dir = Some(".deepagents/deepagents/skills".into());
            global.subagent_dir = Some(".deepagents/deepagents/agents".into());
            global.subagent_ext = "AGENTS.md".into();
            global.mcp = Some(path(".deepagents", ".mcp.json"));
            global.hooks = Some(path(".deepagents", "hooks.json"));
        }
        "factorydroid" => {
            project.root_rule = Some(path(".", "AGENTS.md"));
            project.nonroot_rule_dir = Some(".factory/rules".into());
            project.command_dir = Some(".factory/commands".into());
            project.subagent_dir = Some(".factory/droids".into());
            project.skill_dir = Some(".factory/skills".into());
            project.mcp = Some(path(".factory", "mcp.json"));
            project.hooks = Some(path(".factory", "hooks.json"));
            project.permissions = Some(path(".factory", "settings.json"));
            global.root_rule = Some(path(".factory", "AGENTS.md"));
            global.nonroot_rule_dir = None;
            global.command_dir = Some(".factory/commands".into());
            global.subagent_dir = Some(".factory/droids".into());
            global.skill_dir = Some(".factory/skills".into());
            global.mcp = Some(path(".factory", "mcp.json"));
            global.hooks = Some(path(".factory", "hooks.json"));
            global.permissions = Some(path(".factory", "settings.json"));
        }
        "goose" => {
            project.root_rule = Some(path(".", ".goosehints"));
            project.rule_mode = RuleMode::Fold;
            project.subagent_dir = Some(".goose/agents".into());
            project.command_dir = Some(".goose/recipes".into());
            project.command_ext = "yaml".into();
            project.skill_dir = Some(".goose/skills".into());
            project.mcp = Some(path(".agents/plugins/carabiner", ".mcp.json"));
            project.hooks = Some(path(".agents/plugins/carabiner/hooks", "hooks.json"));
            global.root_rule = Some(path(".config/goose", ".goosehints"));
            global.rule_mode = RuleMode::Fold;
            global.command_dir = Some(".config/goose/recipes".into());
            global.command_ext = "yaml".into();
            global.subagent_dir = Some(".config/goose/agents".into());
            global.mcp = Some(path(".config/goose", "config.yaml"));
            global.hooks = Some(path(".agents/plugins/carabiner/hooks", "hooks.json"));
            global.permissions = Some(path(".config/goose", "permission.yaml"));
        }
        "grokcli" => {
            project.root_rule = Some(path(".", "AGENTS.md"));
            project.nonroot_rule_dir = Some(".grok/rules".into());
            project.command_dir = Some(".grok/commands".into());
            project.subagent_dir = Some(".grok/agents".into());
            project.skill_dir = Some(".grok/skills".into());
            project.mcp = Some(path(".grok", "config.toml"));
            project.hooks = Some(path(".grok/hooks", "carabiner.json"));
            project.permissions = Some(path(".grok", "config.toml"));
            global.root_rule = Some(path(".grok", "AGENTS.md"));
            global.nonroot_rule_dir = Some(".grok/rules".into());
            global.command_dir = Some(".grok/commands".into());
            global.subagent_dir = Some(".grok/agents".into());
            global.skill_dir = Some(".grok/skills".into());
            global.mcp = Some(path(".grok", "config.toml"));
            global.hooks = Some(path(".grok/hooks", "carabiner.json"));
            global.permissions = Some(path(".grok", "config.toml"));
        }
        "hermesagent" => {
            project.root_rule = Some(path(".", ".hermes.md"));
            project.subagent_dir = Some(".hermes/carabiner/subagents".into());
            project.subagent_ext = "json".into();
            project.rule_mode = RuleMode::Fold;
            global.root_rule = Some(path(".", ".hermes.md"));
            global.rule_mode = RuleMode::Fold;
            global.command_dir = Some(".hermes/carabiner/commands".into());
            global.command_ext = "json".into();
            global.subagent_dir = Some(".hermes/carabiner/subagents".into());
            global.subagent_ext = "json".into();
            global.skill_dir = Some(".hermes/skills".into());
            global.mcp = Some(path(".hermes", "config.yaml"));
            global.hooks = Some(path(".hermes", "config.yaml"));
            global.permissions = Some(path(".hermes", "config.yaml"));
            project.checks = Some(path(
                ".hermes/plugins/carabiner-checks/checks",
                "__dynamic__",
            ));
            project.ignore = Some(path(
                ".hermes/plugins/carabiner-ignore",
                "patterns.gitignore",
            ));
        }
        "junie" => {
            project.root_rule = Some(path(".junie", "AGENTS.md"));
            project.rule_mode = RuleMode::Fold;
            project.command_dir = Some(".junie/commands".into());
            project.subagent_dir = Some(".junie/agents".into());
            project.skill_dir = Some(".junie/skills".into());
            project.mcp = Some(path(".junie/mcp", "mcp.json"));
            project.ignore = Some(path(".", ".aiignore"));
            project.hooks = None;
            project.permissions = None;
            global.root_rule = Some(path(".junie", "AGENTS.md"));
            global.hooks = Some(path(".junie", "config.json"));
            global.command_dir = Some(".junie/commands".into());
            global.subagent_dir = Some(".junie/agents".into());
            global.skill_dir = Some(".junie/skills".into());
            global.mcp = Some(path(".junie/mcp", "mcp.json"));
            global.permissions = Some(path(".junie", "allowlist.json"));
        }
        "kilo" => {
            project.root_rule = Some(path(".", "AGENTS.md"));
            project.nonroot_rule_dir = Some(".kilo/rules".into());
            project.command_dir = Some(".kilo/commands".into());
            project.subagent_dir = Some(".kilo/agents".into());
            project.skill_dir = Some(".kilo/skills".into());
            project.mcp = Some(path(".", "kilo.json"));
            project.hooks = Some(path(".kilo/plugins", "carabiner-hooks.js"));
            project.permissions = Some(path(".", "kilo.json"));
            project.ignore = Some(path(".", ".kilocodeignore"));
            global.root_rule = Some(path(".config/kilo", "AGENTS.md"));
            global.nonroot_rule_dir = Some(".kilo/rules".into());
            global.command_dir = Some(".config/kilo/commands".into());
            global.subagent_dir = Some(".config/kilo/agents".into());
            global.skill_dir = Some(".kilo/skills".into());
            global.mcp = Some(path(".config/kilo", "kilo.json"));
            global.hooks = Some(path(".config/kilo/plugins", "carabiner-hooks.js"));
            global.permissions = Some(path(".config/kilo", "kilo.json"));
        }
        "kimi-code" => {
            project.root_rule = Some(path(".kimi-code", "AGENTS.md"));
            project.rule_mode = RuleMode::Fold;
            project.subagent_dir = Some(".kimi-code/agents".into());
            project.skill_dir = Some(".kimi-code/skills".into());
            project.mcp = Some(path(".kimi-code", "mcp.json"));
            global.root_rule = Some(path(".kimi-code", "AGENTS.md"));
            global.rule_mode = RuleMode::Fold;
            global.subagent_dir = Some(".kimi-code/agents".into());
            global.skill_dir = Some(".kimi-code/skills".into());
            global.mcp = Some(path(".kimi-code", "mcp.json"));
            global.hooks = Some(path(".kimi-code", "config.toml"));
            global.permissions = Some(path(".kimi-code", "config.toml"));
        }
        "qwencode" => {
            project.root_rule = Some(path(".", "QWEN.md"));
            project.nonroot_rule_dir = Some(".qwen/rules".into());
            project.local_rule = Some(path(".qwen", "QWEN.local.md"));
            project.command_dir = Some(".qwen/commands".into());
            project.subagent_dir = Some(".qwen/agents".into());
            project.skill_dir = Some(".qwen/skills".into());
            project.mcp = Some(path(".qwen", "settings.json"));
            project.hooks = Some(path(".qwen", "settings.json"));
            project.permissions = Some(path(".qwen", "settings.json"));
            project.ignore = Some(path(".", ".qwenignore"));
            global.root_rule = Some(path(".qwen", "QWEN.md"));
            global.nonroot_rule_dir = Some(".qwen/rules".into());
            global.command_dir = Some(".qwen/commands".into());
            global.subagent_dir = Some(".qwen/agents".into());
            global.skill_dir = Some(".qwen/skills".into());
            global.mcp = Some(path(".qwen", "settings.json"));
            global.hooks = Some(path(".qwen", "settings.json"));
            global.permissions = Some(path(".qwen", "settings.json"));
        }
        "kiro" | "kiro-cli" | "kiro-ide" => {
            project.root_rule = Some(path(".", "AGENTS.md"));
            project.nonroot_rule_dir = Some(".kiro/steering".into());
            project.command_dir = Some(".kiro/prompts".into());
            project.subagent_dir = Some(".kiro/agents".into());
            project.skill_dir = Some(".kiro/skills".into());
            project.mcp = Some(path(".kiro/settings", "mcp.json"));
            project.hooks = Some(path(".kiro/hooks", "carabiner.json"));
            project.permissions = Some(path(".kiro/settings", "permissions.json"));
            project.ignore = Some(path(".", ".kiroignore"));
            global.root_rule = Some(path(".kiro/steering", "product.md"));
            global.nonroot_rule_dir = Some(".kiro/steering".into());
            global.command_dir = Some(".kiro/prompts".into());
            global.subagent_dir = Some(".kiro/agents".into());
            global.skill_dir = Some(".kiro/skills".into());
            global.mcp = Some(path(".kiro/settings", "mcp.json"));
            global.hooks = Some(path(".kiro/hooks", "carabiner.json"));
            global.permissions = Some(path(".kiro/settings", "permissions.json"));
            global.ignore = Some(path(".kiro/settings", "kiroignore"));
        }
        "musecode" => {
            project.root_rule = Some(path(".", "AGENTS.md"));
            project.rule_mode = RuleMode::Fold;
            project.skill_dir = Some(".agents/skills".into());
            global.skill_dir = Some(".config/muse/skills".into());
            global.mcp = Some(path(".config/muse", "settings.json"));
        }
        "opencode" => {
            project.root_rule = Some(path(".", "AGENTS.md"));
            project.nonroot_rule_dir = Some(".opencode/memories".into());
            project.command_dir = Some(".opencode/commands".into());
            project.subagent_dir = Some(".opencode/agents".into());
            project.skill_dir = Some(".opencode/skills".into());
            project.mcp = Some(path(".", "opencode.json"));
            project.hooks = Some(path(".opencode/plugins", "carabiner-hooks.js"));
            project.permissions = Some(path(".", "opencode.json"));
            global.root_rule = Some(path(".config/opencode", "AGENTS.md"));
            global.nonroot_rule_dir = Some(".config/opencode/memories".into());
            global.command_dir = Some(".config/opencode/commands".into());
            global.subagent_dir = Some(".config/opencode/agents".into());
            global.skill_dir = Some(".config/opencode/skills".into());
            global.mcp = Some(path(".config/opencode", "opencode.json"));
            global.hooks = Some(path(".config/opencode/plugins", "carabiner-hooks.js"));
            global.permissions = Some(path(".config/opencode", "opencode.json"));
        }
        "pi" => {
            project.root_rule = Some(path(".", "AGENTS.md"));
            project.rule_mode = RuleMode::Fold;
            project.command_dir = Some(".pi/prompts".into());
            project.skill_dir = Some(".pi/skills".into());
            project.hooks = Some(path(".pi/extensions", "carabiner-hooks.ts"));
            project.permissions = Some(path(".pi", "settings.json"));
            global.root_rule = Some(path(".pi/agent", "AGENTS.md"));
            global.rule_mode = RuleMode::Fold;
            global.command_dir = Some(".pi/agent/prompts".into());
            global.skill_dir = Some(".pi/agent/skills".into());
            global.hooks = Some(path(".pi/agent/extensions", "carabiner-hooks.ts"));
            global.permissions = Some(path(".pi/agent", "settings.json"));
        }
        "reasonix" => {
            project.root_rule = Some(path(".", "REASONIX.md"));
            project.rule_mode = RuleMode::Fold;
            project.nonroot_rule_dir = None;
            project.command_dir = Some(".reasonix/commands".into());
            project.subagent_dir = Some(".reasonix/skills".into());
            project.subagent_ext = "SKILL.md".into();
            project.skill_dir = Some(".reasonix/skills".into());
            project.mcp = Some(path(".", "reasonix.toml"));
            project.hooks = Some(path(".reasonix", "settings.json"));
            project.permissions = Some(path(".", "reasonix.toml"));
            project.ignore = Some(path(".", "reasonix.toml"));
            global.root_rule = Some(path(".reasonix", "REASONIX.md"));
            global.rule_mode = RuleMode::Fold;
            global.nonroot_rule_dir = None;
            global.command_dir = Some(".reasonix/commands".into());
            global.subagent_dir = Some(".reasonix/skills".into());
            global.subagent_ext = "SKILL.md".into();
            global.skill_dir = Some(".reasonix/skills".into());
            global.mcp = Some(path(".reasonix", "config.toml"));
            global.hooks = Some(path(".reasonix", "settings.json"));
            global.permissions = Some(path(".reasonix", "config.toml"));
            global.ignore = Some(path(".reasonix", "config.toml"));
        }
        "replit" => {
            project.root_rule = Some(path(".", "replit.md"));
            project.rule_mode = RuleMode::Fold;
            project.skill_dir = Some(".agents/skills".into());
            global.skill_dir = Some(".agents/skills".into());
        }
        "roo" | "zoocode" => {
            project.root_rule = Some(path(".", "AGENTS.md"));
            project.nonroot_rule_dir = Some(".roo/rules".into());
            project.local_rule = Some(path(".", "AGENTS.local.md"));
            project.command_dir = Some(".roo/commands".into());
            project.subagent_dir = Some(".".into());
            project.subagent_ext = ".roomodes".into();
            project.aggregate_subagents = true;
            project.skill_dir = Some(".roo/skills".into());
            project.mcp = Some(path(".roo", "mcp.json"));
            project.ignore = Some(path(".", ".rooignore"));
            global.root_rule = Some(path(".", "AGENTS.md"));
            global.nonroot_rule_dir = Some(".roo/rules".into());
            global.command_dir = Some(".roo/commands".into());
            global.subagent_dir = Some(".".into());
            global.subagent_ext = ".roomodes".into();
            global.aggregate_subagents = true;
            global.skill_dir = Some(".roo/skills".into());
            global.mcp = Some(path(".roo", "mcp.json"));
            global.ignore = Some(path(".", ".rooignore"));
        }
        "rovodev" => {
            project.root_rule = Some(path(".rovodev", "AGENTS.md"));
            project.nonroot_rule_dir = Some(".rovodev/.carabiner/modular-rules".into());
            project.local_rule = Some(path(".", "AGENTS.local.md"));
            project.command_dir = Some(".rovodev/prompts".into());
            project.subagent_dir = Some(".rovodev/subagents".into());
            project.skill_dir = Some(".rovodev/skills".into());
            project.mcp = Some(path(".rovodev", "mcp.json"));
            project.permissions = Some(path(".rovodev", "config.yml"));
            project.checks = Some(path(".rovodev", ".review-agent.md"));
            global.root_rule = Some(path(".rovodev", "AGENTS.md"));
            global.command_dir = Some(".rovodev/prompts".into());
            global.subagent_dir = Some(".rovodev/subagents".into());
            global.skill_dir = Some(".rovodev/skills".into());
            global.mcp = Some(path(".rovodev", "mcp.json"));
            global.permissions = Some(path(".rovodev", "config.yml"));
        }
        "takt" => {
            project.nonroot_rule_dir = Some(".takt/facets/policies".into());
            project.command_dir = Some(".takt/facets/instructions".into());
            project.subagent_dir = Some(".takt/facets/personas".into());
            project.skill_dir = Some(".takt/facets/knowledge".into());
            project.mcp = Some(path(".takt", "config.yaml"));
            project.permissions = Some(path(".takt", "config.yaml"));
            project.checks = Some(path(".takt", "config.yaml"));
            global.root_rule = Some(path(".takt/facets/policies", "overview.md"));
            global.nonroot_rule_dir = None;
            global.command_dir = Some(".takt/facets/instructions".into());
            global.subagent_dir = Some(".takt/facets/personas".into());
            global.skill_dir = Some(".takt/facets/knowledge".into());
            global.mcp = Some(path(".takt", "config.yaml"));
            global.permissions = Some(path(".takt", "config.yaml"));
            global.checks = Some(path(".takt", "config.yaml"));
        }
        "vibe" => {
            project.root_rule = Some(path(".", "AGENTS.md"));
            project.rule_mode = RuleMode::Fold;
            project.skill_dir = Some(".vibe/skills".into());
            project.subagent_dir = Some(".vibe/agents".into());
            project.subagent_ext = "toml".into();
            project.mcp = Some(path(".vibe", "config.toml"));
            project.hooks = Some(path(".vibe", "hooks.toml"));
            project.permissions = Some(path(".vibe", "config.toml"));
            project.ignore = Some(path(".", ".vibeignore"));
            global.root_rule = Some(path(".vibe", "AGENTS.md"));
            global.rule_mode = RuleMode::Fold;
            global.skill_dir = Some(".vibe/skills".into());
            global.subagent_dir = Some(".vibe/agents".into());
            global.subagent_ext = "toml".into();
            global.mcp = Some(path(".vibe", "config.toml"));
            global.hooks = Some(path(".vibe", "hooks.toml"));
            global.permissions = Some(path(".vibe", "config.toml"));
        }
        "warp" => {
            project.root_rule = Some(path(".", "AGENTS.md"));
            project.mcp = Some(path(".warp", ".mcp.json"));
            project.rule_mode = RuleMode::Fold;
            project.skill_dir = Some(".warp/skills".into());
            project.command_dir = Some(".warp/skills".into());
            project.ignore = Some(path(".", ".warpindexingignore"));
            global.root_rule = Some(path(".agents", "AGENTS.md"));
            global.rule_mode = RuleMode::Fold;
            global.skill_dir = Some(".warp/skills".into());
            global.command_dir = Some(".warp/skills".into());
            global.mcp = Some(path(".warp", ".mcp.json"));
            global.permissions = Some(path(".warp", "settings.toml"));
        }
        "devin" => {
            project.root_rule = Some(path(".", "AGENTS.md"));
            project.nonroot_rule_dir = Some(".devin/rules".into());
            project.local_rule = Some(path(".", "AGENTS.local.md"));
            project.command_dir = Some(".devin/skills".into());
            project.command_ext = "SKILL.md".into();
            project.subagent_dir = Some(".devin/agents".into());
            project.subagent_ext = "AGENT.md".into();
            project.skill_dir = Some(".devin/skills".into());
            project.mcp = Some(path(".devin", "mcp_config.json"));
            project.hooks = Some(path(".devin", "hooks.v1.json"));
            project.permissions = Some(path(".devin", "config.json"));
            project.ignore = Some(path(".", ".devinignore"));
            global.root_rule = Some(path(".config/devin", "AGENTS.md"));
            global.nonroot_rule_dir = Some(".devin/rules".into());
            global.command_dir = Some(".config/devin/skills".into());
            global.command_ext = "SKILL.md".into();
            global.subagent_dir = Some(".config/devin/agents".into());
            global.subagent_ext = "AGENT.md".into();
            global.skill_dir = Some(".config/devin/skills".into());
            global.mcp = Some(path(".config/devin", "mcp_config.json"));
            global.hooks = Some(path(".config/devin", "config.json"));
            global.permissions = Some(path(".config/devin", "config.json"));
            global.ignore = Some(path(".codeium", ".codeiumignore"));
        }
        "zed" => {
            project.root_rule = Some(path(".", ".rules"));
            project.rule_mode = RuleMode::Fold;
            project.skill_dir = Some(".agents/skills".into());
            project.mcp = Some(path(".zed", "settings.json"));
            project.permissions = Some(path(".zed", "settings.json"));
            project.ignore = Some(path(".zed", "settings.json"));
            global.root_rule = Some(path(".config/zed", "AGENTS.md"));
            global.rule_mode = RuleMode::Fold;
            global.skill_dir = Some(".agents/skills".into());
            global.mcp = Some(path(".config/zed", "settings.json"));
            global.permissions = Some(path(".config/zed", "settings.json"));
            global.ignore = Some(path(".config/zed", "settings.json"));
        }
        _ => {}
    }
    if name == "kiro" {
        global.command_dir = None;
        global.subagent_dir = None;
        global.skill_dir = None;
        global.hooks = None;
        global.permissions = None;
        project.subagent_ext = "json".into();
        project.hooks = Some(path(".kiro/agents", "default.json"));
        project.permissions = Some(path(".kiro/agents", "default.json"));
    } else if name == "kiro-cli" {
        project.subagent_ext = "json".into();
        global.subagent_ext = "json".into();
        project.hooks = Some(path(".kiro/hooks", "carabiner.json"));
        global.hooks = Some(path(".kiro/hooks", "carabiner.json"));
        project.permissions = Some(path(".kiro/agents", "default.json"));
        global.permissions = None;
    } else if name == "kiro-ide" {
        project.subagent_ext = "md".into();
        global.subagent_ext = "md".into();
        project.hooks = Some(path(".kiro/hooks", "carabiner.json"));
        global.hooks = Some(path(".kiro/hooks", "carabiner.json"));
        project.permissions = Some(path(".kiro/agents", "default.json"));
        global.permissions = None;
    } else if name == "zoocode" {
        project.permissions = Some(path(".vscode", "settings.json"));
    }
    if name == "hermesagent"
        && std::env::var_os("HERMES_HOME")
            .and_then(|value| (!value.is_empty()).then_some(value))
            .is_some()
    {
        strip_profile_prefix_from_paths(&mut global, ".hermes");
    }
    if name == "kimi-code"
        && std::env::var_os("KIMI_CODE_HOME")
            .and_then(|value| (!value.is_empty()).then_some(value))
            .is_some()
    {
        strip_profile_prefix_from_paths(&mut global, ".kimi-code");
    }
    (project, global)
}

fn feature_sets(name: &str) -> (HashSet<Feature>, HashSet<Feature>, HashSet<Feature>) {
    let project = match name {
        "agentsmd" => set_features(&["rules", "commands", "subagents", "skills"]),
        "agentsskills" => set_features(&["skills"]),
        "aiassistant" => set_features(&["rules", "ignore", "mcp", "skills"]),
        "amp" => set_features(&["rules", "mcp", "skills", "hooks", "permissions", "checks"]),
        "antigravity-cli" => set_features(&[
            "rules",
            "ignore",
            "mcp",
            "commands",
            "subagents",
            "skills",
            "hooks",
        ]),
        "antigravity-ide" => set_features(&[
            "rules",
            "mcp",
            "commands",
            "subagents",
            "skills",
            "hooks",
            "permissions",
        ]),
        "antigravity-plugin" => set_features(&["rules", "mcp", "subagents", "skills", "hooks"]),
        "augmentcode" => set_features(&[
            "rules",
            "ignore",
            "mcp",
            "commands",
            "subagents",
            "skills",
            "hooks",
            "permissions",
            "checks",
        ]),
        "augmentcode-legacy" => set_features(&["rules"]),
        "claudecode" => set_features(&[
            "rules",
            "ignore",
            "mcp",
            "commands",
            "subagents",
            "skills",
            "hooks",
            "permissions",
        ]),
        "claudecode-legacy" => {
            set_features(&["rules", "ignore", "mcp", "commands", "subagents", "skills"])
        }
        "claudecode-plugin" => set_features(&["mcp", "commands", "subagents", "skills", "hooks"]),
        "cline" => set_features(&[
            "rules",
            "ignore",
            "commands",
            "subagents",
            "skills",
            "hooks",
            "permissions",
        ]),
        "codexcli" => set_features(&[
            "rules",
            "mcp",
            "commands",
            "subagents",
            "skills",
            "hooks",
            "permissions",
        ]),
        "copilot" => set_features(&[
            "rules",
            "mcp",
            "commands",
            "subagents",
            "skills",
            "hooks",
            "permissions",
        ]),
        "copilotcli" => set_features(&[
            "rules",
            "mcp",
            "subagents",
            "skills",
            "hooks",
            "permissions",
        ]),
        "cursor" => set_features(&[
            "rules",
            "ignore",
            "mcp",
            "commands",
            "subagents",
            "skills",
            "hooks",
            "permissions",
            "checks",
        ]),
        "deepagents" => set_features(&["rules", "mcp", "subagents", "skills", "hooks"]),
        "factorydroid" => set_features(&[
            "rules",
            "mcp",
            "commands",
            "subagents",
            "skills",
            "hooks",
            "permissions",
        ]),
        "goose" => set_features(&["rules", "mcp", "commands", "subagents", "skills", "hooks"]),
        "grokcli" => set_features(&[
            "rules",
            "mcp",
            "commands",
            "subagents",
            "skills",
            "hooks",
            "permissions",
        ]),
        "hermesagent" => set_features(&["rules", "ignore", "subagents", "checks"]),
        "junie" => set_features(&["rules", "ignore", "mcp", "commands", "subagents", "skills"]),
        "kilo" => set_features(&[
            "rules",
            "ignore",
            "mcp",
            "commands",
            "subagents",
            "skills",
            "hooks",
            "permissions",
        ]),
        "kimi-code" => set_features(&["rules", "mcp", "subagents", "skills"]),
        "kiro" | "kiro-cli" | "kiro-ide" => set_features(&[
            "rules",
            "ignore",
            "mcp",
            "commands",
            "subagents",
            "skills",
            "hooks",
            "permissions",
        ]),
        "musecode" => set_features(&["rules", "skills"]),
        "opencode" => set_features(&[
            "rules",
            "mcp",
            "commands",
            "subagents",
            "skills",
            "hooks",
            "permissions",
        ]),
        "pi" => set_features(&["rules", "commands", "skills", "hooks", "permissions"]),
        "qwencode" => set_features(&[
            "rules",
            "ignore",
            "mcp",
            "commands",
            "subagents",
            "skills",
            "hooks",
            "permissions",
        ]),
        "reasonix" => set_features(&[
            "rules",
            "ignore",
            "mcp",
            "commands",
            "subagents",
            "skills",
            "hooks",
            "permissions",
        ]),
        "replit" => set_features(&["rules", "skills"]),
        "roo" => set_features(&["rules", "ignore", "mcp", "commands", "subagents", "skills"]),
        "zoocode" => set_features(&[
            "rules",
            "ignore",
            "mcp",
            "commands",
            "subagents",
            "skills",
            "permissions",
        ]),
        "rovodev" => set_features(&[
            "rules",
            "mcp",
            "commands",
            "subagents",
            "skills",
            "permissions",
            "checks",
        ]),
        "takt" => set_features(&[
            "rules",
            "mcp",
            "commands",
            "subagents",
            "skills",
            "permissions",
            "checks",
        ]),
        "vibe" => set_features(&[
            "rules",
            "ignore",
            "mcp",
            "subagents",
            "skills",
            "hooks",
            "permissions",
        ]),
        "warp" => set_features(&["rules", "ignore", "mcp", "commands", "skills"]),
        "devin" => set_features(&[
            "rules",
            "ignore",
            "mcp",
            "commands",
            "subagents",
            "skills",
            "hooks",
            "permissions",
        ]),
        "zed" => set_features(&["rules", "ignore", "mcp", "skills", "permissions"]),
        _ => HashSet::new(),
    };

    let global = match name {
        "amp" => set_features(&["rules", "mcp", "skills", "hooks", "permissions", "checks"]),
        "agentsskills" => set_features(&["skills"]),
        "aiassistant" => set_features(&["mcp"]),
        "antigravity-cli" => set_features(&[
            "rules",
            "mcp",
            "commands",
            "subagents",
            "skills",
            "hooks",
            "permissions",
        ]),
        "antigravity-ide" => {
            set_features(&["rules", "mcp", "commands", "subagents", "skills", "hooks"])
        }
        "augmentcode" => set_features(&[
            "rules",
            "mcp",
            "commands",
            "subagents",
            "skills",
            "hooks",
            "permissions",
        ]),
        "claudecode" => set_features(&[
            "rules",
            "mcp",
            "commands",
            "subagents",
            "skills",
            "hooks",
            "permissions",
        ]),
        "claudecode-legacy" => set_features(&["rules", "mcp", "commands", "subagents", "skills"]),
        "cline" => set_features(&["rules", "mcp", "commands", "subagents", "skills", "hooks"]),
        "codexcli" => set_features(&[
            "rules",
            "mcp",
            "commands",
            "subagents",
            "skills",
            "hooks",
            "permissions",
        ]),
        "copilot" => set_features(&["rules", "subagents", "skills", "hooks"]),
        "copilotcli" => set_features(&[
            "rules",
            "mcp",
            "subagents",
            "skills",
            "hooks",
            "permissions",
        ]),
        "cursor" => set_features(&[
            "mcp",
            "commands",
            "subagents",
            "skills",
            "hooks",
            "permissions",
        ]),
        "deepagents" => set_features(&["rules", "mcp", "subagents", "skills", "hooks"]),
        "junie" => set_features(&[
            "rules",
            "mcp",
            "commands",
            "subagents",
            "skills",
            "hooks",
            "permissions",
        ]),
        "rovodev" => set_features(&[
            "rules",
            "mcp",
            "commands",
            "subagents",
            "skills",
            "permissions",
        ]),
        "factorydroid" | "grokcli" | "opencode" | "pi" | "reasonix" | "takt" | "devin" | "zed" => {
            project.clone()
        }
        "kimi-code" => set_features(&[
            "rules",
            "mcp",
            "subagents",
            "skills",
            "hooks",
            "permissions",
        ]),
        "kilo" => set_features(&[
            "rules",
            "mcp",
            "commands",
            "subagents",
            "skills",
            "hooks",
            "permissions",
        ]),
        "qwencode" => set_features(&[
            "rules",
            "mcp",
            "commands",
            "subagents",
            "skills",
            "hooks",
            "permissions",
        ]),
        "vibe" => set_features(&[
            "rules",
            "mcp",
            "subagents",
            "skills",
            "hooks",
            "permissions",
        ]),
        "warp" => set_features(&["rules", "mcp", "commands", "skills", "permissions"]),
        "goose" => set_features(&[
            "rules",
            "mcp",
            "commands",
            "subagents",
            "hooks",
            "permissions",
        ]),
        "kiro" => set_features(&["rules", "ignore", "mcp"]),
        "kiro-cli" => set_features(&[
            "rules",
            "ignore",
            "mcp",
            "commands",
            "subagents",
            "skills",
            "hooks",
        ]),
        "kiro-ide" => set_features(&["rules", "ignore", "mcp", "subagents", "skills", "hooks"]),
        "replit" => set_features(&["skills"]),
        "roo" | "zoocode" => set_features(&["rules", "commands", "skills"]),
        "hermesagent" => set_features(&[
            "mcp",
            "commands",
            "subagents",
            "skills",
            "hooks",
            "permissions",
        ]),
        "musecode" => set_features(&["mcp", "skills"]),
        _ => HashSet::new(),
    };

    let simulated = match name {
        "agentsmd" => set_features(&["commands", "subagents", "skills"]),
        _ => HashSet::new(),
    };
    (project, global, simulated)
}

pub fn target_spec(name: &str) -> Option<TargetSpec> {
    if !all_targets().iter().any(|target| target == name) {
        return None;
    }
    let (mut project, mut global) = project_and_global(name);
    let mcp_format = match name {
        "opencode" | "kilo" => DataFormat::JsonOpenCode,
        "copilot" => DataFormat::JsonServers,
        "amp" => DataFormat::JsonAmp,
        "codexcli" => DataFormat::TomlCodex,
        "vibe" => DataFormat::TomlVibe,
        "grokcli" => DataFormat::TomlGrok,
        "reasonix" => DataFormat::TomlReasonix,
        "goose" => DataFormat::JsonMcpServers,
        "hermesagent" => DataFormat::YamlHermes,
        "takt" => DataFormat::YamlTakt,
        _ => DataFormat::JsonMcpServers,
    };
    project.mcp_format = mcp_format;
    global.mcp_format = if name == "goose" {
        DataFormat::YamlGoose
    } else {
        mcp_format
    };
    let (project_features, global_features, simulated) = feature_sets(name);
    Some(TargetSpec {
        name: name.to_owned(),
        project,
        global,
        project_features,
        global_features,
        simulated,
    })
}

impl TargetSpec {
    pub fn paths(&self, global: bool) -> &ScopePaths {
        if global {
            &self.global
        } else {
            &self.project
        }
    }

    pub fn supports(&self, feature: Feature, global: bool, simulation: bool) -> bool {
        let set = if global {
            &self.global_features
        } else {
            &self.project_features
        };
        if !set.contains(&feature) {
            return false;
        }
        if simulation || !self.simulated.contains(&feature) {
            return true;
        }
        false
    }

    pub fn feature_is_simulated(&self, feature: Feature) -> bool {
        self.simulated.contains(&feature)
    }
}
