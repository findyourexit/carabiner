# Takt

[Takt](https://github.com/nrslib/takt) is an AI coding workflow tool that organizes prompts into facets. Carabiner writes the supported Takt facet files and configuration fields.

## Facet output

In project mode, Carabiner maps its source features to Takt facet directories as follows.

| Carabiner feature | Takt facet directory |
| --- | --- |
| `rules` | `.takt/facets/policies/` by default, or `.takt/facets/output-contracts/` with `takt.facet: output-contracts` |
| `commands` | `.takt/facets/instructions/` |
| `subagents` | `.takt/facets/personas/` |
| `skills` | `.takt/facets/knowledge/` |

Facet files contain plain Markdown, not YAML frontmatter. Carabiner drops source frontmatter when it writes a Takt facet. Rule, command, and skill bodies are otherwise retained, apart from an optional inheritance directive. Subagent bodies are trimmed before they are written.

| Source file | Generated Takt file |
| --- | --- |
| `.carabiner/rules/style.md` | `.takt/facets/policies/style.md` |
| `.carabiner/rules/review-format.md` | `.takt/facets/output-contracts/review-format.md` when `takt.facet: output-contracts` |
| `.carabiner/commands/review.md` | `.takt/facets/instructions/review.md` |
| `.carabiner/subagents/coder.md` | `.takt/facets/personas/coder.md` |
| `.carabiner/skills/oncall/SKILL.md` | `.takt/facets/knowledge/oncall.md` |

Additional files inside a skill source are copied below `.takt/facets/knowledge/` at their path relative to the skill directory.

Global output uses the corresponding `~/.takt/facets/` directories for commands, subagents, and skills. Rules are different in global mode. Carabiner joins all selected non-local rule bodies into `~/.takt/facets/policies/overview.md`, so rule-level `takt.name`, `takt.extends`, and `takt.facet` do not affect global rule output.

## Takt frontmatter

Use a `takt` block in a canonical source file to control its Takt output.

```yaml
---
takt:
  name: my-renamed-stem # rename the emitted filename stem
  extends: base # emit a leading {extends:base} facet-inheritance directive
  facet: output-contracts # rules only, selects the output-contracts facet
---
```

- `takt.name` applies to rules, commands, subagents, and skills. Rules, commands, and subagents otherwise use their source relative path without the `.md` suffix. Skills use the skill name. The resulting name must contain only ASCII letters, digits, `_`, `-`, or `.`, and it cannot be empty, `.`, `..`, or include a path separator. Invalid names fail generation.
- `takt.extends` applies to rules, commands, and skills. A nonempty value writes `{extends:<parent>}` followed by a blank line before the Markdown body. The parent uses the same permitted characters and cannot be `.`, `..`, or include a path separator. Subagents honor `takt.name` but do not process `takt.extends`.
- `takt.facet` applies only to rules. Set it to `output-contracts` to write the rule under `.takt/facets/output-contracts/`. Omit it, or use `policies`, to write under `.takt/facets/policies/`. The commands, subagents, and skills facet directories are fixed.

## MCP transport allowlist

Takt defines concrete `mcp_servers` inside individual workflow steps. Its shared `config.yaml` has no top-level MCP server registry. Carabiner therefore writes only Takt's `workflow_mcp_servers` transport allowlist to `.takt/config.yaml` in project mode and `~/.takt/config.yaml` in global mode.

When generating MCP configuration from `.carabiner/mcp.jsonc`, Carabiner replaces `workflow_mcp_servers` with `stdio`, `sse`, and `http` booleans. A transport is enabled only when a selected server has that exact string in `type`, or in `transport` when `type` is not a string. Use `stdio`, `sse`, or `http` for Takt output. Carabiner does not infer a transport from a command or URL, and values such as `local`, `streamable-http`, and `ws` do not enable an allowlist entry.

Server names, commands, environment variables, URLs, and headers are not representable in this Takt configuration field, so Carabiner does not write them. Define those server details in the relevant Takt workflow step. Carabiner preserves other `config.yaml` fields when it updates the allowlist. Importing this configuration produces an empty `mcpServers` map because the allowlist has no server definitions.

## Permissions

Carabiner writes the Takt permission mode to `provider_profiles.<provider>.default_permission_mode` in the shared `.takt/config.yaml` or `~/.takt/config.yaml`. It uses the top-level `provider` field to select the profile and uses `claude` when that field is absent.

Takt output has one coarse permission mode rather than per-tool or per-pattern rules. Carabiner derives that mode in this order:

1. Any `deny` action produces `readonly`.
2. Otherwise, an allowed `edit` or `write` action produces `edit`.
3. Otherwise, an allowed `bash` action produces `full`.
4. Otherwise, it produces `readonly`.

Actions and patterns that do not affect that decision are not represented in the generated Takt mode. Existing provider-profile fields remain in place apart from `default_permission_mode`.

The optional `takt` block in `.carabiner/permissions.jsonc` can carry configuration that has no canonical permission category. `step_permission_overrides` is written inside the active provider profile. `provider_options`, `network_policy`, `filesystem_policy`, `shell_policy`, and `workflow_command_gates` are written at the top level of `config.yaml`. Carabiner copies those fields when they are present and leaves unrelated existing fields unchanged. Omitting one does not remove an existing value.

On import, `full` becomes an allowed catch-all `bash` permission, `edit` becomes an allowed catch-all `edit` permission, and every other mode becomes a denied catch-all `bash` permission. The Takt-specific override fields above are also imported.

## Checks: quality gates

Checks from `.carabiner/checks/*.md` become Takt quality gates in `workflow_overrides` in the shared `.takt/config.yaml` or `~/.takt/config.yaml`. A check without `takt.command` becomes a string gate. Carabiner uses the trimmed body, then `description`, then the source file stem. A check with `takt.command` becomes a command gate with `type`, `name`, `command`, and optional `cwd` and `timeout_ms` fields. Its body is not used.

```md
---
targets: ["takt"]
takt:
  command: ./.takt/quality-gates/check.sh # omit for a string gate
  timeout_ms: 300000
  steps: ["review"] # optional scope to named workflow steps
  personas: ["coder"] # optional scope to named personas
---
```

For a command gate, `name` defaults to the source file stem. Each name in `steps` and `personas` receives a copy of the gate. If any selected check sets `quality_gates_edit_only: true`, Carabiner writes `workflow_overrides.quality_gates_edit_only: true` for the entire generated configuration. This setting is not scoped to an individual gate.

Takt runs a command gate after an applicable step and fails the gate on a nonzero exit status. Takt's `workflow_command_gates.custom_scripts` policy does not apply to command gates in `workflow_overrides`, so review the frontmatter of a check obtained with `carabiner fetch` before generation.

Takt quality gates have no `severity` or `tools` fields. Carabiner does not write those check fields, and they do not return on import. When at least one check targets Takt, Carabiner replaces `workflow_overrides.quality_gates`, `workflow_overrides.quality_gates_edit_only`, `workflow_overrides.steps`, and `workflow_overrides.personas` from the selected checks. Other top-level configuration fields and `workflow_overrides` keys outside those fields are preserved. When no check targets Takt, Carabiner does not update `config.yaml`, so existing quality gates remain until they are removed manually.

## Importing Takt output

Carabiner can import Takt rules, commands, subagents, checks, and permissions. Takt policy, instruction, and persona facets are plain Markdown, so their content and file path are imported but Takt-specific frontmatter cannot be reconstructed. In particular, `takt.name` and `takt.extends` are not recovered. Files under `.takt/facets/output-contracts/` are imported as rules with `takt.facet: output-contracts`.

Takt skills cannot be imported. If `.takt/facets/knowledge/` contains files and `skills` is selected for import, Carabiner stops with an error rather than creating incomplete skill sources.

Use `carabiner import --targets takt --features checks` to import quality gates into `.carabiner/checks/`. Import assigns generated `takt-<index>.md` source names. String gates import as wildcard-targeted checks. Command gates retain their Takt scope and command fields. The MCP allowlist imports as an empty `mcpServers` map because it has no concrete server definitions. Permission import follows the coarse-mode mapping described above.
