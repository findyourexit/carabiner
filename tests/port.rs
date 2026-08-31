use carabiner::config::ConfigOptions;
use carabiner::engine::{
    export_canonical_to_tool_directory, generate, import_from_tool, ConvertOptions,
    GenerateOptions, ImportOptions,
};
use carabiner::model::{FeatureResult, GenerateResult, ImportResult};
use carabiner::util::{parse_frontmatter, parse_jsonc};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_project(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path =
        std::env::temp_dir().join(format!("carabiner-{label}-{}-{stamp}", std::process::id()));
    fs::create_dir_all(&path).unwrap();
    path
}

fn config(cwd: &Path, targets: &[&str], features: &[&str]) -> GenerateOptions {
    GenerateOptions {
        cwd: Some(cwd.to_path_buf()),
        targets: Some(targets.iter().map(|value| (*value).to_owned()).collect()),
        features: Some(features.iter().map(|value| (*value).to_owned()).collect()),
        ..GenerateOptions::default()
    }
}

#[test]
fn parses_jsonc_comments_and_trailing_commas() {
    let value = parse_jsonc(
        r#"{
      // ignored comment
      "server": {"command": "node",},
    }"#,
    )
    .unwrap();
    assert_eq!(value["server"]["command"], "node");
}

#[test]
fn repairs_colons_and_unquoted_globs_in_frontmatter() {
    let project = temp_project("frontmatter");
    let path = project.join("rule.md");
    let parsed = parse_frontmatter(
        "---\ndescription: Use this when: the user asks\nglobs: *.rs\n---\n\nBody\n",
        &path,
    )
    .unwrap();
    assert_eq!(parsed.data["description"], "Use this when: the user asks");
    assert_eq!(parsed.data["globs"], "*.rs");
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn generates_rules_for_claude_and_cursor() {
    let project = temp_project("rules");
    fs::create_dir_all(project.join(".carabiner/rules")).unwrap();
    fs::write(
        project.join(".carabiner/rules/overview.md"),
        "---\nroot: true\ntargets: [\"*\"]\ndescription: Overview\nglobs: [\"**/*\"]\n---\n\n# Overview\n",
    )
    .unwrap();
    let result = generate(config(&project, &["claudecode", "cursor"], &["rules"])).unwrap();
    assert_eq!(result.rules.count, 2);
    assert_eq!(
        fs::read_to_string(project.join("CLAUDE.md"))
            .unwrap()
            .trim(),
        "# Overview"
    );
    let cursor = fs::read_to_string(project.join(".cursor/rules/overview.mdc")).unwrap();
    assert!(cursor.contains("description: Overview"));
    assert!(cursor.contains("# Overview"));
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn generates_pi_default_tools_preserving_settings() {
    let project = temp_project("pi-permissions");
    fs::create_dir_all(project.join(".pi")).unwrap();
    fs::write(
        project.join(".pi/settings.json"),
        r#"{"theme":"dark","defaultModel":"sonnet"}"#,
    )
    .unwrap();
    fs::create_dir_all(project.join(".carabiner")).unwrap();
    fs::write(
        project.join(".carabiner/permissions.jsonc"),
        r#"{"permission":{},"pi":{"defaultTools":["bash","edit"]}}"#,
    )
    .unwrap();

    let result = generate(config(&project, &["pi"], &["permissions"])).unwrap();
    assert_eq!(result.permissions.count, 1);
    let settings: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(project.join(".pi/settings.json")).unwrap())
            .unwrap();
    assert_eq!(
        settings["defaultTools"],
        serde_json::json!(["bash", "edit"])
    );
    assert_eq!(settings["theme"], "dark");
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn later_input_root_replaces_same_rule() {
    let project = temp_project("overlay");
    let base = project.join("base");
    let overlay = project.join("overlay");
    fs::create_dir_all(base.join("rules")).unwrap();
    fs::create_dir_all(overlay.join("rules")).unwrap();
    let source =
        |body: &str| format!("---\nroot: true\ntargets: [\"claudecode\"]\n---\n\n{body}\n");
    fs::write(base.join("rules/overview.md"), source("base")).unwrap();
    fs::write(overlay.join("rules/overview.md"), source("overlay")).unwrap();
    let mut options = config(&project, &["claudecode"], &["rules"]);
    options.input_roots = Some(vec![
        base.to_string_lossy().into_owned(),
        overlay.to_string_lossy().into_owned(),
    ]);
    generate(options).unwrap();
    assert_eq!(
        fs::read_to_string(project.join("CLAUDE.md"))
            .unwrap()
            .trim(),
        "overlay"
    );
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn imports_cursor_rule_into_carabiner_source_tree() {
    let project = temp_project("import");
    fs::create_dir_all(project.join(".cursor/rules")).unwrap();
    fs::write(
        project.join(".cursor/rules/security.mdc"),
        "---\nalwaysApply: false\ndescription: Security\nglobs: src/**\n---\n\nUse secure defaults.\n",
    )
    .unwrap();
    let options = ImportOptions::from_config(config(&project, &["cursor"], &["rules"]));
    let result = import_from_tool(options).unwrap();
    assert_eq!(result.rules, 1);
    let imported = fs::read_to_string(project.join(".carabiner/rules/security.md")).unwrap();
    assert!(imported.contains("targets:"));
    assert!(imported.contains("Use secure defaults."));
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn rejects_input_root_traversal() {
    let project = temp_project("security");
    let mut options = config(&project, &["claudecode"], &["rules"]);
    options.input_roots = Some(vec!["../outside".into()]);
    let error = generate(options).unwrap_err().to_string();
    assert!(error.contains("path traversal"));
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn shared_feature_outputs_are_idempotent() {
    let project = temp_project("idempotent-shared");
    fs::create_dir_all(project.join(".carabiner/rules")).unwrap();
    fs::write(
        project.join(".carabiner/rules/overview.md"),
        "---\nroot: true\ntargets: [\"cline\"]\n---\n\n# Overview\n",
    )
    .unwrap();
    fs::write(
        project.join(".carabiner/hooks.jsonc"),
        "{\"hooks\":{\"postToolUse\":[{\"matcher\":\"Write\",\"command\":\"echo format\"}]}}",
    )
    .unwrap();
    let mut options = config(&project, &["cline"], &["rules", "hooks"]);
    options.delete = Some(true);
    let first = generate(options.clone()).unwrap();
    let second = generate(options).unwrap();
    assert!(first.total_files() > 0);
    assert_eq!(second.total_files(), 0);
    assert!(project
        .join(".clinerules/hooks/carabiner-hooks.json")
        .is_file());
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn rovo_permissions_do_not_duplicate_on_regeneration() {
    let project = temp_project("idempotent-rovodev");
    fs::create_dir_all(project.join(".carabiner")).unwrap();
    fs::write(
        project.join(".carabiner/permissions.jsonc"),
        "{\"permission\":{\"bash\":{\"git status\":\"allow\",\"rm -rf *\":\"deny\"}}}",
    )
    .unwrap();
    let options = config(&project, &["rovodev"], &["permissions"]);
    generate(options.clone()).unwrap();
    let second = generate(options).unwrap();
    assert_eq!(second.total_files(), 0);
    let content = fs::read_to_string(project.join(".rovodev/config.yml")).unwrap();
    assert_eq!(content.matches("command: git status").count(), 1);
    assert_eq!(content.matches("command: rm -rf *").count(), 1);
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn local_source_install_preserves_first_source_ownership() {
    let project = temp_project("source-ownership");
    let first = project.join("first");
    let second = project.join("second");
    for source in [&first, &second] {
        fs::create_dir_all(source.join("rules")).unwrap();
        fs::create_dir_all(source.join("skills/shared")).unwrap();
    }
    fs::write(
        first.join("rules/shared.md"),
        "---\ntargets: [\"*\"]\n---\nfirst rule\n",
    )
    .unwrap();
    fs::write(
        second.join("rules/shared.md"),
        "---\ntargets: [\"*\"]\n---\nsecond rule\n",
    )
    .unwrap();
    fs::write(
        first.join("skills/shared/SKILL.md"),
        "---\nname: shared\ndescription: first\ntargets: [\"*\"]\n---\nfirst skill\n",
    )
    .unwrap();
    fs::write(
        second.join("skills/shared/SKILL.md"),
        "---\nname: shared\ndescription: second\ntargets: [\"*\"]\n---\nsecond skill\n",
    )
    .unwrap();
    fs::create_dir_all(project.join(".carabiner")).unwrap();
    let config_value = serde_json::json!({
        "sources": [
            {"source": first.to_string_lossy(), "skills": ["*"], "rules": ["*"]},
            {"source": second.to_string_lossy(), "skills": ["*"], "rules": ["*"]}
        ]
    });
    fs::write(
        project.join("carabiner.jsonc"),
        serde_json::to_string_pretty(&config_value).unwrap(),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_carabiner"))
        .args(["--json", "install", "--silent"])
        .current_dir(&project)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        fs::read_to_string(project.join(".carabiner/rules/.curated/shared.md"))
            .unwrap()
            .contains("first rule")
    );
    assert!(
        fs::read_to_string(project.join(".carabiner/skills/.curated/shared/SKILL.md"))
            .unwrap()
            .contains("description: first")
    );
    let lock: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(project.join("carabiner.lock")).unwrap()).unwrap();
    assert!(lock["sources"]
        .as_object()
        .unwrap()
        .values()
        .any(|entry| entry["rules"]["shared"].is_object()));
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn imports_kiro_embedded_hooks_without_permission_leak() {
    let project = temp_project("kiro-hooks-import");
    fs::create_dir_all(project.join(".kiro/agents")).unwrap();
    fs::write(
        project.join(".kiro/agents/default.json"),
        r#"{
  "hooks": {
    "preToolUse": [{"command": "echo guard", "matcher": "Write"}]
  },
  "toolsSettings": {
    "shell": {"allowedCommands": ["git status"], "deniedCommands": []}
  }
}"#,
    )
    .unwrap();
    let config = ConfigOptions {
        cwd: Some(project.clone()),
        targets: Some(vec!["kiro".into()]),
        features: Some(vec!["hooks".into()]),
        ..ConfigOptions::default()
    };
    let result = import_from_tool(ImportOptions::from_config(config)).unwrap();
    assert_eq!(result.hooks, 1);
    let hooks =
        parse_jsonc(&fs::read_to_string(project.join(".carabiner/hooks.jsonc")).unwrap()).unwrap();
    assert_eq!(hooks["version"], 1);
    assert_eq!(hooks["hooks"]["preToolUse"][0]["type"], "command");
    assert!(hooks.get("toolsSettings").is_none());
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn imports_codex_workspace_denials_as_read_and_edit_rules() {
    let project = temp_project("codex-permissions-import");
    fs::create_dir_all(project.join(".codex")).unwrap();
    fs::write(
        project.join(".codex/config.toml"),
        r#"default_permissions = "carabiner"

[permissions.carabiner]
extends = ":workspace"

[permissions.carabiner.filesystem]
":minimal" = "read"

[permissions.carabiner.filesystem.":workspace_roots"]
"secrets/**" = "deny"
".git/**" = "write"
"#,
    )
    .unwrap();
    let config = ConfigOptions {
        cwd: Some(project.clone()),
        targets: Some(vec!["codexcli".into()]),
        features: Some(vec!["permissions".into()]),
        ..ConfigOptions::default()
    };
    let result = import_from_tool(ImportOptions::from_config(config)).unwrap();
    assert_eq!(result.permissions, 1);
    let permissions =
        parse_jsonc(&fs::read_to_string(project.join(".carabiner/permissions.jsonc")).unwrap())
            .unwrap();
    assert_eq!(permissions["permission"]["read"]["secrets/**"], "deny");
    assert_eq!(permissions["permission"]["edit"]["secrets/**"], "deny");
    assert!(permissions["permission"]["edit"].get(".git/**").is_none());
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn imports_opencode_inline_entries_into_namespaced_sources() {
    let project = temp_project("opencode-inline-import");
    fs::create_dir_all(project.join("prompts")).unwrap();
    fs::write(
        project.join("prompts/reviewer.md"),
        "Review instructions loaded from a file.\n",
    )
    .unwrap();
    fs::write(
        project.join("opencode.json"),
        r#"{
  "command": {
    "review": {
      "description": "Review command",
      "agent": "planner",
      "model": "gpt-5",
      "subtask": true,
      "template": "Run the review."
    }
  },
  "agent": {
    "reviewer": {
      "description": "Review agent",
      "model": "gpt-5",
      "prompt": "{file:./prompts/reviewer.md}"
    }
  }
}"#,
    )
    .unwrap();
    let config = ConfigOptions {
        cwd: Some(project.clone()),
        targets: Some(vec!["opencode".into()]),
        features: Some(vec!["commands".into(), "subagents".into()]),
        ..ConfigOptions::default()
    };
    let result = import_from_tool(ImportOptions::from_config(config)).unwrap();
    assert_eq!(result.commands, 1);
    assert_eq!(result.subagents, 1);

    let command = parse_frontmatter(
        &fs::read_to_string(project.join(".carabiner/commands/review.md")).unwrap(),
        &project.join(".carabiner/commands/review.md"),
    )
    .unwrap();
    assert_eq!(command.data["opencode"]["agent"], "planner");
    assert!(command.data.get("agent").is_none());

    let agent_path = project.join(".carabiner/subagents/reviewer.md");
    let agent = parse_frontmatter(&fs::read_to_string(&agent_path).unwrap(), &agent_path).unwrap();
    assert_eq!(agent.data["opencode"]["model"], "gpt-5");
    assert_eq!(agent.data["opencode"]["mode"], "subagent");
    assert!(agent.data.get("model").is_none());
    assert_eq!(agent.body.trim(), "Review instructions loaded from a file.");
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn exposes_flat_public_results_and_input_root_inspection() {
    let generated = GenerateResult {
        rules: FeatureResult {
            count: 2,
            paths: vec!["CLAUDE.md".into(), ".cursor/rules/overview.mdc".into()],
        },
        skill_details: vec![serde_json::json!({"name": "demo"})],
        has_diff: true,
        ..GenerateResult::default()
    };
    let flat_generated = generated.flat();
    assert_eq!(flat_generated.rules_count, 2);
    assert_eq!(
        serde_json::to_value(&flat_generated).unwrap()["rulesCount"],
        2
    );
    assert_eq!(flat_generated.skills, generated.skill_details);
    assert!(flat_generated.has_diff);

    let imported = ImportResult {
        rules: 1,
        ..ImportResult::default()
    };
    assert_eq!(imported.flat().rules_count, 1);
    assert_eq!(
        serde_json::to_value(imported.flat()).unwrap()["rulesCount"],
        1
    );
    let import_options = ImportOptions {
        target: "cursor".into(),
        features: Some(vec!["rules".into()]),
        ..ImportOptions::default()
    };
    assert_eq!(
        import_options
            .to_config_options()
            .targets
            .as_deref()
            .unwrap(),
        ["cursor".to_owned()]
    );
    let convert_options = ConvertOptions {
        from: "cursor".into(),
        to: vec!["claudecode".into()],
        features: Some(vec!["rules".into()]),
        ..ConvertOptions::default()
    };
    assert_eq!(
        convert_options.to_config_options().features.unwrap(),
        vec!["rules".to_owned()]
    );

    let existing = temp_project("input-root-inspection-existing");
    let missing = existing.join("missing");
    let inspection = carabiner::inspect_input_roots(&[existing.clone(), missing.clone()]);
    assert_eq!(inspection.existing, vec![existing.clone()]);
    assert_eq!(inspection.missing, vec![missing]);
    assert!(inspection.message.is_none());
    fs::remove_dir_all(existing).unwrap();
}

#[test]
fn exports_canonical_sources_to_native_tool_layout() {
    let root = temp_project("canonical-export");
    let source = root.join("source");
    let staging = root.join("staging");
    fs::create_dir_all(source.join("rules")).unwrap();
    fs::create_dir_all(&staging).unwrap();
    fs::write(
        source.join("rules/guide.md"),
        "---\ntargets: [\"*\"]\ndescription: Guide\nglobs: [\"src/**\"]\n---\nCanonical source\n",
    )
    .unwrap();
    let result =
        export_canonical_to_tool_directory("cursor", &source, &staging, &["rules".into()]).unwrap();
    assert_eq!(result.rules.count, 1);
    assert!(staging.join(".cursor/rules/guide.mdc").is_file());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn fetch_converts_canonical_sources_for_native_targets() {
    let root = temp_project("native-fetch");
    let source = root.join("source");
    let project = root.join("project");
    fs::create_dir_all(source.join("rules")).unwrap();
    fs::create_dir_all(&project).unwrap();
    fs::write(
        source.join("rules/guide.md"),
        "---\ntargets: [\"*\"]\ndescription: Guide\nglobs: [\"src/**\"]\n---\nCanonical input\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_carabiner"))
        .args([
            "fetch",
            source.to_string_lossy().as_ref(),
            "--target",
            "cursor",
            "--features",
            "rules",
            "--output",
            ".carabiner",
            "--silent",
        ])
        .current_dir(&project)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        project.join(".carabiner/rules/guide.md").is_file(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn fetch_reports_file_statuses() {
    let root = temp_project("fetch-statuses");
    let source = root.join("source");
    let project = root.join("project");
    fs::create_dir_all(source.join("rules")).unwrap();
    fs::create_dir_all(&project).unwrap();
    fs::write(
        source.join("rules/guide.md"),
        "---\ntargets: [\"*\"]\n---\nGuide\n",
    )
    .unwrap();
    let run = |conflict: &str| {
        let output = Command::new(env!("CARGO_BIN_EXE_carabiner"))
            .args([
                "--json",
                "fetch",
                source.to_string_lossy().as_ref(),
                "--target",
                "carabiner",
                "--features",
                "rules",
                "--output",
                ".carabiner",
                "--conflict",
                conflict,
            ])
            .current_dir(&project)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
    };
    let created = run("overwrite");
    assert_eq!(
        created["data"]["created"],
        serde_json::json!(["rules/guide.md"])
    );
    assert_eq!(created["data"]["totalFetched"], 1);
    let skipped = run("skip");
    assert_eq!(
        skipped["data"]["skipped"],
        serde_json::json!(["rules/guide.md"])
    );
    fs::write(
        source.join("rules/guide.md"),
        "---\ntargets: [\"*\"]\n---\nUpdated\n",
    )
    .unwrap();
    let overwritten = run("overwrite");
    assert_eq!(
        overwritten["data"]["overwritten"],
        serde_json::json!(["rules/guide.md"])
    );
    fs::remove_dir_all(root).unwrap();
}
