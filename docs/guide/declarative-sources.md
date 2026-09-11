# Declarative Sources

Carabiner can fetch rules and skills from external repositories using the `install` command. Instead of manually running `fetch` for each source, declare it in your `carabiner.jsonc` and run `carabiner install` to resolve and fetch its selected artifacts. Then `carabiner generate` processes them as curated inputs. Typical workflow: `carabiner install && carabiner generate`.

To add one source without editing JSONC by hand, run `carabiner add <source>`. It preserves existing comments, appends the source entry, installs it, and updates the appropriate lockfile:

```bash
carabiner add anthropics/skills --skills skill-creator

# Add one rule without selecting any skills
carabiner add acme/ai-standards --rules testing-guidelines
```

The command fetches only the source being added. Existing sources must already be locked and installed; run `carabiner install` first when they are not. If the new source fails, Carabiner restores the manifest, source lockfiles, curated rules, and curated skills to their previous state.

## Configuration

Add a `sources` array to your `carabiner.jsonc`:

```jsonc title="carabiner.jsonc"
{
  "$schema": "https://github.com/findyourexit/carabiner/releases/latest/download/config-schema.json",
  "targets": ["copilot", "claudecode"],
  "features": ["rules", "skills"],
  "sources": [
    // All skills from a GitHub source.
    { "source": "owner/repo" },

    // Selected skills.
    { "source": "anthropics/skills", "skills": ["skill-creator"] },

    // Selected Markdown rules.
    {
      "source": "acme/ai-standards",
      "rules": ["testing-guidelines", "typescript-conventions"],
    },

    // Rules and skills from one source.
    {
      "source": "acme/ai-assets",
      "rules": ["*"],
      "rulesPath": "exports/rules",
      "skills": ["review-pr"],
      "path": "exports/skills",
    },

    // A pinned source subdirectory.
    { "source": "owner/repo@v1.0.0:path/to/skills" },

    // Git source.
    {
      "source": "https://dev.azure.com/org/project/_git/repo",
      "transport": "git",
      "ref": "main",
      "path": "exports/skills",
    },

    // Local Git source.
    { "source": "file:///path/to/local/repo", "transport": "git" },

    // Git source with a root SKILL.md.
    {
      "source": "https://github.com/feature-sliced/skills",
      "transport": "git",
      "path": ".",
    },

    // npm package source.
    {
      "source": "@acme/skill-package",
      "transport": "npm",
      "registry": "https://acme.jfrog.io/artifactory/api/npm/npm-local/",
      "tokenEnv": "ACME_REGISTRY_TOKEN",
    },
  ],
}
```

Each entry in `sources` accepts:

| Property    | Type       | Description                                                                                                                                                                                                           |
| ----------- | ---------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `source`    | `string`   | Repository source. For GitHub transport: `owner/repo` or `owner/repo@ref:path`. For git transport: a native path or `https://`, `ssh://`, `git://`, `git@host:path`, or `file://` URL. For npm transport: a package name (`pkg` or `@scope/pkg`). |
| `skills`    | `string[]` | Optional skill names to fetch. `"*"` selects all skills. When both `skills` and `rules` are omitted, all skills are fetched for backward compatibility.                                                               |
| `rules`     | `string[]` | Optional rule names to fetch. Names may include or omit `.md`; `"*"` selects every direct `.md` file under `rulesPath`. Setting only `rules` fetches no skills.                                                       |
| `transport` | `string`   | `"github"` (default) uses the GitHub REST API. `"git"` uses git CLI and works with any git remote. `"npm"` (experimental) fetches a package from an npm-compatible registry.                                          |
| `ref`       | `string`   | Branch, tag, or ref to fetch from. Defaults to the remote's default branch. For GitHub transport, use the `@ref` source syntax. For npm transport: an exact version or dist-tag (defaults to `latest`).               |
| `path`      | `string`   | Path to the skills directory within the repository. Defaults to `"skills"`. Set to `""`, `"."`, or `"./"` to target the entire repository root (see note below). For GitHub transport, use the `:path` source syntax. |
| `rulesPath` | `string`   | Path to the rules directory within the repository or package. Defaults to `"rules"`. This is independent from the skills-only `path` field.                                                                           |
| `registry`  | `string`   | npm transport only. HTTPS base URL of the npm-compatible registry. Defaults to `https://registry.npmjs.org`. |
| `tokenEnv`  | `string`   | npm transport only. Name of the environment variable holding the registry token. `NPM_TOKEN` is allowed by default; other names require a local allowlist. |

Rules are flat source files: only direct `.md` children of `rulesPath` are discovered. Nested rule files are not installed. Fetched rules are written to `.carabiner/rules/.curated/<rule-name>.md`; during generation they behave as if they were ordinary files directly under `.carabiner/rules/`.

!!! info "Repository-root paths (`path: "."`)"
    When `path` is `""`, `"."`, or `"./"` (with the `git` transport), Carabiner fetches the entire repository tree, then groups each top-level directory as a skill. This is useful for single-skill repositories whose `SKILL.md` lives at the repo root (`<repo>/SKILL.md`) rather than under a `skills/` container. Prefer a narrower `path` for large repositories.

## npm Transport (Experimental)

!!! warning
    The `npm` transport is **experimental**. Its configuration surface and lockfile format may change in a future release.

The `npm` transport fetches skills from any registry that implements the npm registry API. Because JFrog Artifactory, Sonatype Nexus, Verdaccio, GitHub Packages, and similar private registries all expose an npm-compatible API, a single transport with a configurable `registry` URL covers them all. This lets enterprises whose build environments cannot reach public GitHub distribute skills internally as npm packages.

Only HTTPS registry and tarball URLs are accepted. Authenticated npm requests do not follow redirects, so private registries must return their metadata and tarball directly from the configured origin.

How a package is fetched:

1. The package metadata (packument) is fetched from `<registry>/<package>` using the abbreviated `application/vnd.npm.install-v1+json` form.
2. The declared `ref` (an **exact version** or a **dist-tag** such as `latest` or `beta` — semver ranges are not supported) is resolved to a concrete version.
3. The version's tarball is downloaded and must verify against the registry's `dist.integrity` or `dist.shasum` metadata.
4. The tarball is parsed by an in-process hardened reader into an isolated staging directory. Only regular files are materialized; symlinks, hardlinks, devices, duplicate paths, traversal, and overly deep paths are rejected. Extraction is capped at 10,000 files and 100 MiB of decompressed content.

Package layout: skills are discovered the same way as for the git transports. Skill directories under `skills/` (or the configured `path`) are installed as `.carabiner/skills/.curated/<name>/`. Direct `.md` files under `rules/` (or the configured `rulesPath`) can be selected with `rules` and are installed under `.carabiner/rules/.curated/`. A single-skill package with `SKILL.md` at the package root is installed as one skill named after the package's base name (`@acme/my-skill` installs as `my-skill`); note that this root fallback installs the package's root-level files only, so prefer the `skills/<name>/` layout for skills that carry subdirectories such as `references/`.

Authentication uses a bearer token from an environment variable: `NPM_TOKEN` is allowed for the default npmjs origin. To use a token with a private registry, explicitly add the registry origin to `CARABINER_NPM_TRUSTED_REGISTRIES`; custom `tokenEnv` names must also appear in `CARABINER_NPM_TOKEN_ENV_ALLOWLIST`. Both variables accept comma-separated values. Tokens are sent only to the configured tarball origin and authenticated requests reject redirects. `.npmrc` files are intentionally **not** read.

Resolved versions are pinned in `carabiner-npm.lock.json` (next to `carabiner.lock`), which records the resolved version, the tarball integrity, and per-artifact content hashes. Commit it for reproducible installs. `--frozen` verifies the cached artifacts against those hashes, never contacts a remote or registry, and fails if the cache is missing or differs from the lockfile; run a normal `carabiner install` to restore the cache.

## How It Works

```mermaid
flowchart LR
    sources["Sources in carabiner.jsonc"] --> install["carabiner install resolves refs"]
    install --> artifacts["Artifacts written to .curated/ directories"]
    artifacts --> generate["carabiner generate uses them"]
```

When `carabiner install` runs and `sources` is configured:

1. **Lockfile resolution** — A normal install resolves each source ref to a commit SHA and records it in `carabiner.lock`; npm sources record a resolved version and tarball integrity in `carabiner-npm.lock.json`. `--frozen` validates the committed lockfile and cached artifacts without resolving or fetching any source.
2. **Remote artifact listing** — The configured skills and rules directories are listed from the remote source.
3. **Filtering** — Only the names selected by `skills` and `rules` are fetched. Omitting both fields retains the historical behavior of fetching all skills.
4. **Precedence rules**:
   - **Local inputs win within one source tree** — Rules and skills outside `.curated/` take precedence over a same-named curated artifact in that input root. Across multiple `inputRoots`, root order remains primary: a later root replaces an earlier root's effective artifact even when the later artifact is curated.
   - **First-declared source wins** — If two sources provide an artifact with the same name, the one declared first in the `sources` array is used.
5. **Output** — Fetched rules are written to `.carabiner/rules/.curated/<rule-name>.md`; fetched skills are written to `.carabiner/skills/.curated/<skill-name>/`. Both directories are automatically added to `.gitignore` by `carabiner gitignore`.

## Install Modes

`carabiner install` supports three install modes via `--mode <mode>`:

| Mode       | Manifest input               | Lockfile                                                     | Output layout                                                                                                      |
| ---------- | ---------------------------- | ------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `carabiner` | `carabiner.jsonc` `sources`   | `carabiner.lock` (+ `carabiner-npm.lock.json` for npm sources) | `.carabiner/rules/.curated/<name>.md`, `.carabiner/skills/.curated/<name>/` (then re-emitted by `carabiner generate`) |
| `apm`      | `apm.yml` `dependencies.apm` | `carabiner-apm.lock.yaml`                                     | `.github/instructions/`, `.github/skills/` (APM v1 layout)                                                         |
| `gh`       | `carabiner.jsonc` `sources`   | `carabiner-gh.lock.yaml`                                      | Per-agent / per-scope dirs (matching `gh skill install`)                                                           |

=== "carabiner mode"

    ```bash
    carabiner install
    ```

=== "gh mode"

    ```bash
    carabiner install --mode gh
    ```

=== "apm mode"

    ```bash
    carabiner install --mode apm
    ```

When `--mode` is omitted, Carabiner defaults to `carabiner` mode. If `apm.yml` is present and `sources` is also defined, you must pass `--mode apm` or `--mode carabiner` to disambiguate.

### `--mode gh` — gh-skill-install–compatible layout

`--mode gh` reads the same `sources` array from `carabiner.jsonc` but writes each discovered skill into the agent-specific directory expected by `gh skill install`. Each source supports two extra fields:

| Property | Type     | Default          | Description                                                                               |
| -------- | -------- | ---------------- | ----------------------------------------------------------------------------------------- |
| `agent`  | `string` | `github-copilot` | One of `github-copilot`, `claude-code`, `cursor`, `codex`, `gemini`, `antigravity`.       |
| `scope`  | `string` | `project`        | `project` writes inside the project root; `user` writes inside the user's home directory. |

Agent → install directory mapping:

| Agent            | Project scope (relative to project root) | User scope (relative to home) |
| ---------------- | ---------------------------------------- | ----------------------------- |
| `github-copilot` | `.agents/skills`                         | `.copilot/skills`             |
| `claude-code`    | `.claude/skills`                         | `.claude/skills`              |
| `cursor`         | `.agents/skills`                         | `.cursor/skills`              |
| `codex`          | `.agents/skills`                         | `.agents/skills`              |
| `gemini`         | `.agents/skills`                         | `.gemini/skills`              |
| `antigravity`    | `.agents/skills`                         | `.gemini/antigravity/skills`  |

For each skill discovered as `skills/<name>/SKILL.md` in the remote repository, Carabiner deploys the entire skill directory to `<install-dir>/<name>/` and injects a provenance frontmatter block (`source`, `repository`, `ref`) into the deployed `SKILL.md`. The lockfile `carabiner-gh.lock.yaml` records one entry per `(source, agent, scope, skill)` tuple.

Per-source field support in `--mode gh`:

| Field       | Status                                                                                                                                       |
| ----------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| `source`    | Required. Must resolve to a GitHub repository (`owner/repo`, `owner/repo@ref`, or an `https://github.com/...` URL).                          |
| `skills`    | Optional. When set, only the listed skill names are installed; remote skills not in the list are skipped, and missing names log a warning.   |
| `rules`     | **Rejected.** Declarative rules are supported only in `--mode carabiner`.                                                                     |
| `rulesPath` | **Rejected.** Declarative rules are supported only in `--mode carabiner`.                                                                     |
| `ref`       | Optional. Pins a tag, branch, or commit SHA. When omitted, gh mode resolves to the latest release's tag, falling back to the default branch. |
| `agent`     | Optional. Defaults to `github-copilot`. See the agent table above.                                                                           |
| `scope`     | Optional. Defaults to `project`.                                                                                                             |
| `transport` | **Rejected.** gh mode is GitHub-only and does not honor the `git` transport. Drop the field or switch to `--mode carabiner`.                  |
| `path`      | **Rejected.** The remote layout is fixed to `skills/<name>/SKILL.md`. Repositories that store skills elsewhere are not supported in gh mode. |

The remote repository must use the layout `skills/<name>/SKILL.md` (one directory per skill, each containing a `SKILL.md`). Other layouts are not auto-discovered.

Example `carabiner.jsonc`:

```jsonc title="carabiner.jsonc"
{
  "targets": ["claudecode"],
  "features": ["rules"],
  "sources": [
    // Default GitHub Copilot project path: .agents/skills/git-commit/.
    { "source": "acme/skills", "skills": ["git-commit"] },

    // Claude Code user path: ~/.claude/skills/git-commit/.
    {
      "source": "acme/skills",
      "skills": ["git-commit"],
      "agent": "claude-code",
      "scope": "user",
    },
  ],
}
```

Run with `carabiner install --mode gh`.

## CLI Options

The `install` command accepts these flags:

| Flag              | Description                                                                                                                                                          |
| ----------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `--mode <mode>`   | Install mode: `carabiner` (default), `apm`, or `gh`. See **Install Modes** above.                                                                                     |
| `--update`        | Force re-resolve all source refs, ignoring the lockfile (useful to pull new updates).                                                                                |
| `--frozen`        | Require a complete lockfile and matching cached artifacts. It never resolves or fetches sources and fails if the cache is missing or differs from the lockfile. Useful for CI. |
| `--token <token>` | Explicit GitHub or npm token. For HTTPS GitHub sources, it overrides `GITHUB_TOKEN` and `GH_TOKEN`; for npm sources without `tokenEnv`, it overrides `NPM_TOKEN`. |

```bash
# Install rules and skills using locked refs
carabiner install

# Force update to latest refs
carabiner install --update

# CI mode with locked cached sources.
carabiner install --frozen

# Install then generate
carabiner install && carabiner generate

# Generate without installing sources.
carabiner generate
```

## Lockfile

The lockfile at `carabiner.lock` (at the project root) records the resolved commit SHA, rule selection metadata, and per-artifact integrity hashes for each source so that builds are reproducible. Carabiner verifies cached rule content against these hashes before reusing it. It is safe to commit this file. An example:

```json title="carabiner.lock"
{
  "lockfileVersion": 1,
  "sources": {
    "owner/skill-repo": {
      "requestedRef": "main",
      "resolvedRef": "abc123def456...",
      "resolvedAt": "2025-01-15T12:00:00.000Z",
      "skills": {
        "my-skill": { "integrity": "sha256-abcdef..." },
        "another-skill": { "integrity": "sha256-123456..." }
      },
      "rules": {
        "testing-guidelines": { "integrity": "sha256-789abc..." }
      },
      "ruleSelection": ["*"],
      "rulesPath": "rules",
      "resolvedRuleNames": ["testing-guidelines"]
    }
  }
}
```

To update locked refs, run `carabiner install --update`.

npm-transport sources (experimental) are pinned in a separate `carabiner-npm.lock.json`, because they lock a resolved package version and tarball integrity instead of a commit SHA:

```json title="carabiner-npm.lock.json"
{
  "lockfileVersion": 1,
  "sources": {
    "@acme/skill-package": {
      "registry": "https://acme.jfrog.io/artifactory/api/npm/npm-local",
      "requestedVersion": "latest",
      "resolvedVersion": "1.2.3",
      "integrity": "sha512-...",
      "resolvedAt": "2026-01-15T12:00:00.000Z",
      "skills": {
        "my-skill": { "integrity": "sha256-abcdef..." }
      },
      "rules": {
        "testing-guidelines": { "integrity": "sha256-789abc..." }
      },
      "ruleSelection": ["testing-guidelines"],
      "rulesPath": "rules",
      "resolvedRuleNames": ["testing-guidelines"]
    }
  }
}
```

It is safe (and recommended) to commit this file as well.

## Authentication

GitHub transport uses the `GITHUB_TOKEN` or `GH_TOKEN` environment variable for authentication. For an HTTPS `github.com` source, Carabiner supplies the token through Git's per-process configuration rather than embedding it in the clone URL. The explicit `--token` flag takes precedence over those environment variables. Git transport relies on your local git credential configuration (SSH keys, credential helpers, etc.). npm transport (experimental) uses the variable named by the per-source `tokenEnv` field when present; otherwise it uses `--token`, then `NPM_TOKEN`. `.npmrc` files are not read.

```bash
# Using environment variable
export GITHUB_TOKEN=ghp_xxxx
carabiner install

# Or using GitHub CLI
GITHUB_TOKEN=$(gh auth token) carabiner install
```

!!! tip
    The `install` command also accepts a `--token` flag for explicit authentication: `carabiner install --token ghp_xxxx`.

## Curated vs Local Inputs

| Location                             | Type    | Precedence within one root | Committed to Git |
| ------------------------------------ | ------- | -------------------------- | ---------------- |
| `.carabiner/skills/<name>/`           | Local   | Higher                     | Yes              |
| `.carabiner/skills/.curated/<name>/`  | Curated | Lower                      | No (gitignored)  |
| `.carabiner/rules/<name>.md`          | Local   | Higher                     | Yes              |
| `.carabiner/rules/.curated/<name>.md` | Curated | Lower                      | No (gitignored)  |

When a local and curated artifact in the same source tree share a name, the local artifact is used and the remote one is not fetched. With multiple input roots, this per-root selection happens before the roots are merged in order; see [Separate Input Root](./separate-input-root.md#merge-behavior).
