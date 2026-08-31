# Dry Run

Use `carabiner generate --dry-run` to preview generated changes without changing files. It reports the files that would be written or removed.

## `--dry-run`

```bash
carabiner generate --dry-run --targets claudecode --features rules
```

The preview uses the same configuration and source files as a normal generation run, but it does not write or delete generated files.

## `--check`

Use `--check` in automated checks to confirm that generated files are current. It performs the same no-write comparison and exits with status code `1` when files differ from the generated output.

```bash
carabiner generate --check --targets "*" --features "*"
```

`--dry-run` and `--check` cannot be used together.
