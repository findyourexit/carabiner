use carabiner::engine::{export_canonical_to_tool_directory, import_from_tool, ImportOptions};
use carabiner::model::Feature;
use carabiner::targets::{all_features, all_targets, target_spec};
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

fn write_canonical_fixture(root: &Path) {
    for directory in ["rules", "commands", "subagents", "skills/demo", "checks"] {
        fs::create_dir_all(root.join(directory)).unwrap();
    }
    fs::write(
    root.join("rules/overview.md"),
    "---\nroot: true\ntargets: [\"*\"]\ndescription: Overview\nglobs: [\"**/*\"]\n---\n\n# Overview\n",
  )
  .unwrap();
    fs::write(
        root.join("commands/review.md"),
        "---\ntargets: [\"*\"]\ndescription: Review changes\n---\n\nReview the current changes.\n",
    )
    .unwrap();
    fs::write(
    root.join("subagents/reviewer.md"),
    "---\nname: reviewer\ntargets: [\"*\"]\ndescription: Review specialist\n---\n\nReview the current changes.\n",
  )
  .unwrap();
    fs::write(
    root.join("skills/demo/SKILL.md"),
    "---\nname: demo\ndescription: Demonstration skill\ntargets: [\"*\"]\n---\n\nUse this demonstration skill.\n",
  )
  .unwrap();
    fs::write(
    root.join("checks/style.md"),
    "---\ntargets: [\"*\"]\ndescription: Style check\nseverity: medium\ntools: [\"bash\"]\n---\n\nCheck style.\n",
  )
  .unwrap();
    fs::write(
    root.join("mcp.jsonc"),
    "{\n  \"mcpServers\": {\n    \"demo\": {\n      \"command\": \"echo\",\n      \"args\": [\"demo\"]\n    }\n  }\n}\n",
  )
  .unwrap();
    fs::write(
    root.join("hooks.jsonc"),
    "{\n  \"hooks\": {\n    \"postToolUse\": [{\"matcher\": \"Write\", \"command\": \"echo done\"}]\n  }\n}\n",
  )
  .unwrap();
    fs::write(
    root.join("permissions.jsonc"),
        "{\n  \"permission\": {\n    \"bash\": {\"*\": \"allow\"},\n    \"read\": {\"*\": \"allow\"},\n    \"edit\": {\"*\": \"allow\"},\n    \"webfetch\": {\"https://example.com\": \"allow\"}\n  }\n}\n",
  )
  .unwrap();
    fs::write(root.join(".aiignore"), "target/\n").unwrap();
}

fn supported_features(target: &str, global: bool) -> Vec<String> {
    let spec = target_spec(target).unwrap();
    Feature::ALL
        .into_iter()
        .filter(|feature| spec.supports(*feature, global, false))
        .map(|feature| feature.as_str().to_owned())
        .collect()
}

#[test]
fn every_target_generates_and_imports_project_fixtures() {
    let root = temp_project("compatibility-project");
    let source = root.join("source");
    write_canonical_fixture(&source);
    let all_features = all_features();

    for target in all_targets() {
        let output = root.join("output").join(&target);
        fs::create_dir_all(&output).unwrap();
        let supported = supported_features(&target, false);
        assert!(
            !supported.is_empty(),
            "{target} has no project capabilities"
        );
        let generated =
            export_canonical_to_tool_directory(&target, &source, &output, &all_features)
                .unwrap_or_else(|error| panic!("{target} project generation failed: {error:#}"));
        assert!(
            generated.total_files() > 0,
            "{target} generated no project files for supported features {supported:?}",
        );
        let spec = target_spec(&target).unwrap();
        for feature in Feature::ALL {
            if spec.supports(feature, false, false) {
                assert!(
                    generated.feature(feature).count > 0,
                    "{target} generated no project output for {}",
                    feature.as_str(),
                );
            }
        }

        let mut import_features = all_features.clone();
        if target == "takt" {
            import_features.retain(|feature| feature != "skills");
        }
        let imported = import_from_tool(ImportOptions {
            target: target.clone(),
            features: Some(import_features),
            cwd: Some(output.clone()),
            output_root: Some(output.to_string_lossy().into_owned()),
            silent: Some(true),
            ..ImportOptions::default()
        })
        .unwrap_or_else(|error| panic!("{target} project import failed: {error:#}"));
        assert!(
            imported.total_files() > 0,
            "{target} imported no project files for supported features {supported:?}",
        );
    }

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn every_global_target_generates_in_an_isolated_home() {
    let root = temp_project("compatibility-global");
    let project = root.join("project");
    let source = project.join(".carabiner");
    write_canonical_fixture(&source);

    for target in all_targets() {
        let features = supported_features(&target, true);
        if features.is_empty() {
            continue;
        }
        let home = root.join("homes").join(&target);
        fs::create_dir_all(&home).unwrap();
        let feature_list = features.join(",");
        let output = Command::new(env!("CARGO_BIN_EXE_carabiner"))
            .args([
                "--json",
                "generate",
                "--targets",
                target.as_str(),
                "--features",
                feature_list.as_str(),
                "--global",
                "--silent",
            ])
            .current_dir(&project)
            .env("HOME_DIR", &home)
            .env("HOME", &home)
            .env_remove("HERMES_HOME")
            .env_remove("KIMI_CODE_HOME")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{target} global generation failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(response["success"], true, "{target}: {response}");
        assert!(
            response["data"]["totalFiles"].as_u64().unwrap_or_default() > 0,
            "{target} generated no global files for supported features {features:?}",
        );
        for feature in &features {
            assert!(
                response["data"]["features"][feature]["count"]
                    .as_u64()
                    .unwrap_or_default()
                    > 0,
                "{target} generated no global output for {feature}",
            );
        }
    }

    fs::remove_dir_all(root).unwrap();
}

#[cfg(windows)]
#[test]
fn hermes_project_generation_uses_userprofile_when_home_is_unset() {
    let root = temp_project("compatibility-userprofile");
    let project = root.join("project");
    write_canonical_fixture(&project.join(".carabiner"));
    let userprofile = root.join("userprofile");
    fs::create_dir_all(&userprofile).unwrap();
    let features = supported_features("hermesagent", false).join(",");

    let output = Command::new(env!("CARGO_BIN_EXE_carabiner"))
        .args([
            "--json",
            "generate",
            "--targets",
            "hermesagent",
            "--features",
            features.as_str(),
            "--silent",
        ])
        .current_dir(&project)
        .env("USERPROFILE", &userprofile)
        .env_remove("HOME_DIR")
        .env_remove("HOME")
        .env_remove("HERMES_HOME")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "Hermes project generation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        userprofile.join(".hermes/config.yaml").is_file(),
        "Hermes configuration was not written under USERPROFILE"
    );

    fs::remove_dir_all(root).unwrap();
}
