#[cfg(unix)]
use base64::{engine::general_purpose::STANDARD, Engine as _};
use carabiner::config::ConfigOptions;
use carabiner::engine::{
    export_canonical_to_tool_directory, generate, import_from_tool, ConvertOptions,
    GenerateOptions, ImportOptions,
};
use carabiner::model::{FeatureResult, GenerateResult, ImportResult};
use carabiner::util::{parse_frontmatter, parse_jsonc, write_text};
use std::fs;
#[cfg(unix)]
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
#[cfg(unix)]
use std::process::Stdio;
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

#[cfg(unix)]
fn mcp_call(project: &Path, arguments: serde_json::Value) -> serde_json::Value {
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {"name": "carabinerTool", "arguments": arguments},
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_carabiner"))
        .arg("mcp")
        .current_dir(project)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(stdin, "{}", serde_json::to_string(&request).unwrap()).unwrap();
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
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
fn rejects_unknown_shared_hook_event() {
    let project = temp_project("unknown-hook-event");
    fs::create_dir_all(project.join(".carabiner")).unwrap();
    fs::write(
        project.join(".carabiner/hooks.jsonc"),
        r#"{"hooks":{"unknownEvent":[{"command":"echo ignored"}]}}"#,
    )
    .unwrap();
    let error = generate(config(&project, &["claudecode"], &["hooks"])).unwrap_err();
    assert!(error
        .to_string()
        .contains("Unknown canonical hook event 'unknownEvent'"));
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn rejects_nonobject_shared_hooks() {
    let project = temp_project("nonobject-hooks");
    fs::create_dir_all(project.join(".carabiner")).unwrap();
    fs::write(project.join(".carabiner/hooks.jsonc"), r#"{"hooks":[]}"#).unwrap();

    let error = generate(config(&project, &["claudecode"], &["hooks"])).unwrap_err();
    assert!(error
        .to_string()
        .contains("Canonical hooks source 'hooks' must be an object."));
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

#[cfg(unix)]
#[test]
fn mcp_rejects_symlinked_workspace_paths() {
    let project = temp_project("mcp-symlink");
    let outside = temp_project("mcp-symlink-outside");
    fs::create_dir_all(project.join(".carabiner")).unwrap();
    std::os::unix::fs::symlink(&outside, project.join(".carabiner/rules")).unwrap();

    let write = mcp_call(
        &project,
        serde_json::json!({
            "feature": "rule",
            "operation": "put",
            "targetPathFromCwd": ".carabiner/rules/escape.md",
            "body": "must not escape",
        }),
    );
    assert_eq!(write["result"]["isError"], true);
    assert!(!outside.join("escape.md").exists());

    fs::write(outside.join("victim.md"), "must not delete").unwrap();
    let delete = mcp_call(
        &project,
        serde_json::json!({
            "feature": "rule",
            "operation": "delete",
            "targetPathFromCwd": ".carabiner/rules/victim.md",
        }),
    );
    assert_eq!(delete["result"]["isError"], true);
    assert!(outside.join("victim.md").exists());

    let skill_root = project.join(".carabiner/skills/demo");
    fs::create_dir_all(&skill_root).unwrap();
    std::os::unix::fs::symlink(&outside, skill_root.join("nested")).unwrap();
    let nested_write = mcp_call(
        &project,
        serde_json::json!({
            "feature": "skill",
            "operation": "put",
            "targetPathFromCwd": ".carabiner/skills/demo",
            "frontmatter": {"name": "demo", "description": "demo"},
            "body": "must not partially write",
            "otherFiles": [{"name": "nested/escape.txt", "body": "must not escape"}],
        }),
    );
    assert_eq!(nested_write["result"]["isError"], true);
    assert!(!outside.join("escape.txt").exists());
    assert!(!skill_root.join("SKILL.md").exists());

    fs::remove_dir_all(project).unwrap();
    fs::remove_dir_all(outside).unwrap();
}

#[test]
fn frozen_install_requires_matching_cached_artifacts() {
    let project = temp_project("frozen-install");
    let source = project.join("source");
    fs::create_dir_all(source.join("skills/demo")).unwrap();
    fs::write(
        source.join("skills/demo/SKILL.md"),
        "---\nname: demo\ndescription: Demo\ntargets: [\"*\"]\n---\n\nOriginal\n",
    )
    .unwrap();
    fs::write(
        project.join("carabiner.jsonc"),
        serde_json::json!({"sources": [{"source": source, "transport": "git"}]}).to_string(),
    )
    .unwrap();

    let install = Command::new(env!("CARGO_BIN_EXE_carabiner"))
        .args(["install", "--silent"])
        .current_dir(&project)
        .output()
        .unwrap();
    assert!(
        install.status.success(),
        "{}",
        String::from_utf8_lossy(&install.stderr)
    );
    let lock = fs::read(project.join("carabiner.lock")).unwrap();
    fs::remove_dir_all(&source).unwrap();

    let cached = project.join(".carabiner/skills/.curated/demo/SKILL.md");
    let frozen = Command::new(env!("CARGO_BIN_EXE_carabiner"))
        .args(["install", "--frozen", "--silent"])
        .current_dir(&project)
        .output()
        .unwrap();
    assert!(
        frozen.status.success(),
        "{}",
        String::from_utf8_lossy(&frozen.stderr)
    );
    assert!(cached.is_file());

    fs::write(&cached, "tampered").unwrap();
    let tampered = Command::new(env!("CARGO_BIN_EXE_carabiner"))
        .args(["install", "--frozen", "--silent"])
        .current_dir(&project)
        .output()
        .unwrap();
    assert!(!tampered.status.success());
    assert_eq!(fs::read(project.join("carabiner.lock")).unwrap(), lock);

    fs::remove_dir_all(project).unwrap();
}

#[test]
fn frozen_gh_install_requires_matching_cached_artifacts() {
    let project = temp_project("frozen-gh-install");
    let source = project.join("source");
    fs::create_dir_all(source.join("skills/demo")).unwrap();
    fs::write(
        source.join("skills/demo/SKILL.md"),
        "---\nname: demo\ndescription: Demo\ntargets: [\"*\"]\n---\n\nOriginal\n",
    )
    .unwrap();
    fs::write(
        project.join("carabiner.jsonc"),
        serde_json::json!({"sources": [{"source": source, "skills": ["*"]}]}).to_string(),
    )
    .unwrap();

    let install = Command::new(env!("CARGO_BIN_EXE_carabiner"))
        .args(["install", "--mode", "gh", "--silent"])
        .current_dir(&project)
        .output()
        .unwrap();
    assert!(
        install.status.success(),
        "{}",
        String::from_utf8_lossy(&install.stderr)
    );
    let lock = fs::read(project.join("carabiner-gh.lock.yaml")).unwrap();
    fs::remove_dir_all(&source).unwrap();

    let cached = project.join(".agents/skills/demo/SKILL.md");
    let frozen = Command::new(env!("CARGO_BIN_EXE_carabiner"))
        .args(["install", "--mode", "gh", "--frozen", "--silent"])
        .current_dir(&project)
        .output()
        .unwrap();
    assert!(
        frozen.status.success(),
        "{}",
        String::from_utf8_lossy(&frozen.stderr)
    );
    fs::write(&cached, "tampered").unwrap();

    let tampered = Command::new(env!("CARGO_BIN_EXE_carabiner"))
        .args(["install", "--mode", "gh", "--frozen", "--silent"])
        .current_dir(&project)
        .output()
        .unwrap();
    assert!(!tampered.status.success());
    assert_eq!(
        fs::read(project.join("carabiner-gh.lock.yaml")).unwrap(),
        lock
    );

    fs::remove_dir_all(project).unwrap();
}

#[test]
fn frozen_apm_install_requires_matching_cached_artifacts() {
    let project = temp_project("frozen-apm-install");
    let source = project.join("source");
    fs::create_dir_all(source.join(".apm/instructions")).unwrap();
    fs::create_dir_all(source.join(".apm/skills/demo")).unwrap();
    fs::write(source.join(".apm/instructions/guide.md"), "Original\n").unwrap();
    fs::write(
        source.join(".apm/skills/demo/SKILL.md"),
        "---\nname: demo\ndescription: Demo\ntargets: [\"*\"]\n---\n\nOriginal\n",
    )
    .unwrap();
    fs::write(
        project.join("apm.yml"),
        format!("dependencies:\n  apm:\n    - git: {}\n", source.display()),
    )
    .unwrap();

    let install = Command::new(env!("CARGO_BIN_EXE_carabiner"))
        .args(["install", "--mode", "apm", "--silent"])
        .current_dir(&project)
        .output()
        .unwrap();
    assert!(
        install.status.success(),
        "{}",
        String::from_utf8_lossy(&install.stderr)
    );
    let lock = fs::read(project.join("carabiner-apm.lock.yaml")).unwrap();
    fs::remove_dir_all(&source).unwrap();

    let cached = project.join(".github/instructions/guide.md");
    let frozen = Command::new(env!("CARGO_BIN_EXE_carabiner"))
        .args(["install", "--mode", "apm", "--frozen", "--silent"])
        .current_dir(&project)
        .output()
        .unwrap();
    assert!(
        frozen.status.success(),
        "{}",
        String::from_utf8_lossy(&frozen.stderr)
    );
    fs::write(&cached, "tampered").unwrap();

    let tampered = Command::new(env!("CARGO_BIN_EXE_carabiner"))
        .args(["install", "--mode", "apm", "--frozen", "--silent"])
        .current_dir(&project)
        .output()
        .unwrap();
    assert!(!tampered.status.success());
    assert_eq!(
        fs::read(project.join("carabiner-apm.lock.yaml")).unwrap(),
        lock
    );

    fs::remove_dir_all(project).unwrap();
}

#[test]
fn frozen_npm_install_requires_matching_cached_artifacts() {
    let project = temp_project("frozen-npm-install");
    let cached = project.join(".carabiner/skills/.curated/demo/SKILL.md");
    fs::create_dir_all(cached.parent().unwrap()).unwrap();
    fs::write(
        &cached,
        "---\nname: demo\ndescription: Demo\ntargets: [\"*\"]\n---\n\nOriginal\n",
    )
    .unwrap();
    fs::write(
        project.join("carabiner.jsonc"),
        serde_json::json!({"sources": [{"source": "demo", "transport": "npm"}]}).to_string(),
    )
    .unwrap();
    let lock = serde_json::json!({
        "lockfileVersion": 1,
        "sources": {
            "demo": {
                "resolvedVersion": "1.0.0",
                "integrity": "sha512-fixture",
                "skills": {
                    "demo": {
                        "integrity": "sha256-2cbff3914af684b2bddb9deb708349cc13f9fa82beba086804513ede7bdd21a8"
                    }
                }
            }
        }
    });
    fs::write(
        project.join("carabiner-npm.lock.json"),
        serde_json::to_string_pretty(&lock).unwrap(),
    )
    .unwrap();

    let frozen = Command::new(env!("CARGO_BIN_EXE_carabiner"))
        .args(["install", "--frozen", "--silent"])
        .current_dir(&project)
        .output()
        .unwrap();
    assert!(
        frozen.status.success(),
        "{}",
        String::from_utf8_lossy(&frozen.stderr)
    );

    fs::write(&cached, "tampered").unwrap();
    let tampered = Command::new(env!("CARGO_BIN_EXE_carabiner"))
        .args(["install", "--frozen", "--silent"])
        .current_dir(&project)
        .output()
        .unwrap();
    assert!(!tampered.status.success());

    fs::remove_dir_all(project).unwrap();
}

#[cfg(unix)]
#[test]
fn installs_file_url_git_source() {
    let project = temp_project("file-url-install");
    let source = project.join("source");
    fs::create_dir_all(source.join("skills/demo")).unwrap();
    fs::write(
        source.join("skills/demo/SKILL.md"),
        "---\nname: demo\ndescription: Demo\ntargets: [\"*\"]\n---\n\nFile URL\n",
    )
    .unwrap();
    let run_git = |arguments: &[&str]| {
        let output = Command::new("git")
            .args(arguments)
            .current_dir(&source)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    run_git(&["init"]);
    run_git(&["config", "user.email", "test@example.invalid"]);
    run_git(&["config", "user.name", "Test"]);
    run_git(&["add", "."]);
    run_git(&["-c", "commit.gpgsign=false", "commit", "-m", "initial"]);
    let source_url = format!(
        "file://{}",
        source
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/")
    );
    fs::write(
        project.join("carabiner.jsonc"),
        serde_json::json!({"sources": [{"source": source_url, "transport": "git"}]}).to_string(),
    )
    .unwrap();

    let install = Command::new(env!("CARGO_BIN_EXE_carabiner"))
        .args(["install", "--silent"])
        .current_dir(&project)
        .output()
        .unwrap();
    assert!(
        install.status.success(),
        "{}",
        String::from_utf8_lossy(&install.stderr)
    );
    assert!(project
        .join(".carabiner/skills/.curated/demo/SKILL.md")
        .is_file());

    fs::remove_dir_all(project).unwrap();
}

#[test]
fn fetches_repository_root_skills() {
    let project = temp_project("fetch-root-skills");
    let source = project.join("source");
    fs::create_dir_all(source.join("skills/demo")).unwrap();
    fs::write(
        source.join("skills/demo/SKILL.md"),
        "---\nname: demo\ndescription: Demo\ntargets: [\"*\"]\n---\n\nRoot skill\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_carabiner"))
        .args(["fetch", source.to_str().unwrap(), "--features", "skills"])
        .current_dir(&project)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(project.join(".carabiner/skills/demo/SKILL.md").is_file());

    fs::remove_dir_all(project).unwrap();
}

#[cfg(unix)]
#[test]
fn fetch_authenticates_https_github_sources_without_token_url_leakage() {
    use std::os::unix::fs::PermissionsExt;

    let project = temp_project("github-fetch-auth");
    let bin = project.join("bin");
    let auth_key = project.join("auth-key");
    let auth_value = project.join("auth-value");
    let arguments = project.join("arguments");
    fs::create_dir_all(&bin).unwrap();
    let git = bin.join("git");
    fs::write(
        &git,
        r#"#!/bin/sh
printf '%s\n' "$GIT_CONFIG_KEY_0" > "$AUTH_KEY"
printf '%s\n' "$GIT_CONFIG_VALUE_0" > "$AUTH_VALUE"
printf '%s\n' "$@" > "$ARGUMENTS"
destination=""
for argument in "$@"; do
  destination="$argument"
done
mkdir -p "$destination/skills/demo"
printf '%s\n' '---' 'name: demo' 'description: Demo' 'targets: ["*"]' '---' '' 'Demo' > "$destination/skills/demo/SKILL.md"
"#,
    )
    .unwrap();
    let mut permissions = fs::metadata(&git).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&git, permissions).unwrap();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());

    let output = Command::new(env!("CARGO_BIN_EXE_carabiner"))
        .args(["fetch", "owner/repo", "--token", "cli-token"])
        .current_dir(&project)
        .env("PATH", &path)
        .env("AUTH_KEY", &auth_key)
        .env("AUTH_VALUE", &auth_value)
        .env("ARGUMENTS", &arguments)
        .env("GITHUB_TOKEN", "environment-token")
        .env_remove("GH_TOKEN")
        .env_remove("GIT_CONFIG_COUNT")
        .env_remove("GIT_CONFIG_KEY_0")
        .env_remove("GIT_CONFIG_VALUE_0")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(&auth_key).unwrap().trim(),
        "http.https://github.com/.extraheader"
    );
    assert_eq!(
        fs::read_to_string(&auth_value).unwrap().trim(),
        format!(
            "AUTHORIZATION: Basic {}",
            STANDARD.encode("x-access-token:cli-token")
        )
    );
    assert!(!fs::read_to_string(&arguments)
        .unwrap()
        .contains("cli-token"));
    assert!(project.join(".carabiner/skills/demo/SKILL.md").is_file());

    let output = Command::new(env!("CARGO_BIN_EXE_carabiner"))
        .args(["fetch", "owner/repo"])
        .current_dir(&project)
        .env("PATH", &path)
        .env("AUTH_KEY", &auth_key)
        .env("AUTH_VALUE", &auth_value)
        .env("ARGUMENTS", &arguments)
        .env("GITHUB_TOKEN", "environment-token")
        .env_remove("GH_TOKEN")
        .env_remove("GIT_CONFIG_COUNT")
        .env_remove("GIT_CONFIG_KEY_0")
        .env_remove("GIT_CONFIG_VALUE_0")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(&auth_value).unwrap().trim(),
        format!(
            "AUTHORIZATION: Basic {}",
            STANDARD.encode("x-access-token:environment-token")
        )
    );

    fs::remove_dir_all(project).unwrap();
}

#[cfg(unix)]
#[test]
fn install_uses_github_token_for_clone_and_lock_resolution() {
    use std::os::unix::fs::PermissionsExt;

    let project = temp_project("github-install-auth");
    let bin = project.join("bin");
    let clone_key = project.join("clone-key");
    let clone_auth = project.join("clone-auth");
    let revision_key = project.join("revision-key");
    let revision_auth = project.join("revision-auth");
    fs::create_dir_all(&bin).unwrap();
    fs::write(
        project.join("carabiner.jsonc"),
        r#"{"sources":[{"source":"owner/repo","skills":["demo"]}]}"#,
    )
    .unwrap();
    let git = bin.join("git");
    fs::write(
        &git,
        r#"#!/bin/sh
case "$1" in
  clone)
    printf '%s\n' "$GIT_CONFIG_KEY_0" > "$CLONE_KEY"
    printf '%s\n' "$GIT_CONFIG_VALUE_0" > "$CLONE_AUTH"
    destination=""
    for argument in "$@"; do
      destination="$argument"
    done
    mkdir -p "$destination/skills/demo"
    printf '%s\n' '---' 'name: demo' 'description: Demo' 'targets: ["*"]' '---' '' 'Demo' > "$destination/skills/demo/SKILL.md"
    ;;
  ls-remote)
    printf '%s\n' "$GIT_CONFIG_KEY_0" > "$REVISION_KEY"
    printf '%s\n' "$GIT_CONFIG_VALUE_0" > "$REVISION_AUTH"
    printf '%s\n' '0123456789012345678901234567890123456789 HEAD'
    ;;
  *)
    exit 1
    ;;
esac
"#,
    )
    .unwrap();
    let mut permissions = fs::metadata(&git).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&git, permissions).unwrap();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());

    let output = Command::new(env!("CARGO_BIN_EXE_carabiner"))
        .args(["install", "--token", "cli-token", "--silent"])
        .current_dir(&project)
        .env("PATH", &path)
        .env("CLONE_KEY", &clone_key)
        .env("CLONE_AUTH", &clone_auth)
        .env("REVISION_KEY", &revision_key)
        .env("REVISION_AUTH", &revision_auth)
        .env_remove("GITHUB_TOKEN")
        .env_remove("GH_TOKEN")
        .env_remove("GIT_CONFIG_COUNT")
        .env_remove("GIT_CONFIG_KEY_0")
        .env_remove("GIT_CONFIG_VALUE_0")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected_header = format!(
        "AUTHORIZATION: Basic {}",
        STANDARD.encode("x-access-token:cli-token")
    );
    assert_eq!(
        fs::read_to_string(&clone_key).unwrap().trim(),
        "http.https://github.com/.extraheader"
    );
    assert_eq!(
        fs::read_to_string(&clone_auth).unwrap().trim(),
        expected_header
    );
    assert_eq!(
        fs::read_to_string(&revision_key).unwrap().trim(),
        "http.https://github.com/.extraheader"
    );
    assert_eq!(
        fs::read_to_string(&revision_auth).unwrap().trim(),
        expected_header
    );
    let lock: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(project.join("carabiner.lock")).unwrap()).unwrap();
    assert_eq!(
        lock["sources"]["owner/repo"]["resolvedRef"],
        "0123456789012345678901234567890123456789"
    );

    fs::remove_dir_all(project).unwrap();
}

#[test]
fn generates_documented_canonical_mcp_forms() {
    let project = temp_project("canonical-mcp-forms");
    fs::create_dir_all(project.join(".carabiner")).unwrap();
    fs::write(
        project.join(".carabiner/mcp.jsonc"),
        r#"{
  "mcpServers": {
    "implicit": { "command": "echo", "args": ["implicit"] },
    "implicit-remote": { "url": "https://example.com/implicit" },
    "local": { "type": "local", "command": "echo" },
    "streamable": { "transport": "streamable-http", "httpUrl": "https://example.com/mcp" },
    "array-command": { "command": ["node", "server.js"], "args": ["--log"] },
    "websocket": { "type": "ws", "url": "wss://example.com/mcp" },
    "shared": { "command": "echo" }
  },
  "claudecode": {
    "mcpServers": {
      "shared": null,
      "claude-only": { "command": "echo" }
    }
  }
}"#,
    )
    .unwrap();

    generate(config(&project, &["claudecode"], &["mcp"])).unwrap();
    let generated: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(project.join(".mcp.json")).unwrap()).unwrap();
    let servers = generated["mcpServers"].as_object().unwrap();
    assert_eq!(servers["implicit"]["command"], "echo");
    assert_eq!(servers["implicit"]["type"], "stdio");
    assert_eq!(servers["implicit-remote"]["type"], "http");
    assert_eq!(
        servers["implicit-remote"]["url"],
        "https://example.com/implicit"
    );
    assert_eq!(servers["local"]["type"], "stdio");
    assert_eq!(servers["streamable"]["type"], "streamable-http");
    assert_eq!(servers["streamable"]["url"], "https://example.com/mcp");
    assert!(servers["streamable"].get("transport").is_none());
    assert_eq!(servers["array-command"]["command"], "node");
    assert_eq!(
        servers["array-command"]["args"],
        serde_json::json!(["server.js", "--log"])
    );
    assert_eq!(servers["websocket"]["type"], "ws");
    assert_eq!(servers["claude-only"]["type"], "stdio");
    assert!(servers.contains_key("claude-only"));
    assert!(!servers.contains_key("shared"));

    fs::remove_dir_all(project).unwrap();
}

#[test]
fn preserves_qwen_http_url_mcp_shape() {
    let project = temp_project("qwen-mcp-http-url");
    fs::create_dir_all(project.join(".carabiner")).unwrap();
    fs::write(
        project.join(".carabiner/mcp.jsonc"),
        r#"{"mcpServers":{"remote":{"type":"http","httpUrl":"https://example.com/mcp"}}}"#,
    )
    .unwrap();

    generate(config(&project, &["qwencode"], &["mcp"])).unwrap();
    let generated: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(project.join(".qwen/settings.json")).unwrap())
            .unwrap();
    let server = &generated["mcpServers"]["remote"];
    assert_eq!(server["type"], "http");
    assert_eq!(server["httpUrl"], "https://example.com/mcp");
    assert!(server.get("url").is_none());
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn rejects_ambiguous_mcp_transport_fields() {
    let project = temp_project("ambiguous-mcp-transport");
    fs::create_dir_all(project.join(".carabiner")).unwrap();
    fs::write(
        project.join(".carabiner/mcp.jsonc"),
        r#"{"mcpServers":{"demo":{"type":"stdio","transport":"http","command":"echo","url":"https://example.com/mcp"}}}"#,
    )
    .unwrap();

    let error = generate(config(&project, &["claudecode"], &["mcp"])).unwrap_err();
    assert!(error.to_string().contains(
        "MCP server 'demo' in mcpServers must use either 'type' or 'transport', not both."
    ));
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn rejects_nonobject_tool_scoped_mcp_servers() {
    let project = temp_project("nonobject-tool-mcp");
    fs::create_dir_all(project.join(".carabiner")).unwrap();
    fs::write(
        project.join(".carabiner/mcp.jsonc"),
        r#"{"mcpServers":{},"claudecode":{"mcpServers":[]}}"#,
    )
    .unwrap();

    let error = generate(config(&project, &["claudecode"], &["mcp"])).unwrap_err();
    assert!(error
        .to_string()
        .contains("Canonical MCP source 'claudecode.mcpServers' must be an object."));
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn rejects_invalid_mcp_server_connection_shapes() {
    let project = temp_project("invalid-mcp-connections");
    fs::create_dir_all(project.join(".carabiner")).unwrap();
    for (content, expected) in [
        (
            r#"{"mcpServers":{"demo":{"type":"stdio"}}}"#,
            "with type 'stdio' must define a non-empty command.",
        ),
        (
            r#"{"mcpServers":{"demo":{"transport":"http"}}}"#,
            "with transport 'http' must define a non-empty 'url' or 'httpUrl'.",
        ),
        (
            r#"{"mcpServers":{"demo":{"command":"echo","url":"https://example.com/mcp"}}}"#,
            "must set 'type' or 'transport' when both a command and URL are present.",
        ),
        (
            r#"{"mcpServers":{"demo":{"type":"tcp","command":"echo"}}}"#,
            "field 'type' has unsupported transport 'tcp'.",
        ),
    ] {
        fs::write(project.join(".carabiner/mcp.jsonc"), content).unwrap();
        let error = generate(config(&project, &["claudecode"], &["mcp"])).unwrap_err();
        assert!(error.to_string().contains(expected), "{error:#}");
    }
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn rejects_nonobject_shared_permissions() {
    let project = temp_project("nonobject-shared-permissions");
    fs::create_dir_all(project.join(".carabiner")).unwrap();
    fs::write(
        project.join(".carabiner/permissions.jsonc"),
        r#"{"permission":[]}"#,
    )
    .unwrap();

    let error = generate(config(&project, &["claudecode"], &["permissions"])).unwrap_err();
    assert!(error
        .to_string()
        .contains("Canonical permissions source 'permission' must be an object."));
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn generates_bare_shared_permission_actions() {
    let project = temp_project("bare-shared-permission-action");
    fs::create_dir_all(project.join(".carabiner")).unwrap();
    fs::write(
        project.join(".carabiner/permissions.jsonc"),
        r#"{"permission":{"bash":"allow"}}"#,
    )
    .unwrap();

    generate(config(&project, &["claudecode"], &["permissions"])).unwrap();
    let generated: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(project.join(".claude/settings.json")).unwrap())
            .unwrap();
    assert_eq!(
        generated["permissions"]["allow"],
        serde_json::json!(["Bash"])
    );
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn rejects_malformed_shared_permission_rules() {
    let project = temp_project("malformed-shared-permission-rules");
    fs::create_dir_all(project.join(".carabiner")).unwrap();
    for (content, expected) in [
        (
            r#"{"permission":{"bash":[]}}"#,
            "permission.bash' must be an action string or a map of glob patterns to actions.",
        ),
        (
            r#"{"permission":{"bash":{"git *":1}}}"#,
            "permission.bash.git *' must be 'allow', 'ask', or 'deny'.",
        ),
        (
            r#"{"permission":{"bash":"grant"}}"#,
            "permission.bash' must be 'allow', 'ask', or 'deny'.",
        ),
    ] {
        fs::write(project.join(".carabiner/permissions.jsonc"), content).unwrap();
        let error = generate(config(&project, &["claudecode"], &["permissions"])).unwrap_err();
        assert!(error.to_string().contains(expected), "{error:#}");
    }
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn atomic_text_writes_replace_content_without_temporary_files() {
    let project = temp_project("atomic-write");
    let path = project.join("settings.json");
    assert!(write_text(&path, "first", false).unwrap());
    assert!(write_text(&path, "second", false).unwrap());
    assert!(!write_text(&path, "second", false).unwrap());
    assert_eq!(fs::read_to_string(&path).unwrap(), "second\n");
    assert!(fs::read_dir(&project).unwrap().all(|entry| !entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".carabiner-write-")));
    fs::remove_dir_all(project).unwrap();
}
