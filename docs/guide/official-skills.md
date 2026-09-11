# Official Skills

Carabiner publishes starter skills in the repository's `skills/` directory. Fetch them into a project's `.carabiner/` directory with the `fetch` command.

## Fetch the Collection

```bash
carabiner fetch findyourexit/carabiner
```

The collection currently includes the `project-context` skill. By default, `fetch` requests the `skills` feature and writes fetched files to `.carabiner/`.

## Choose Skills Interactively

Use an interactive terminal to choose individual skills from the collection:

```bash
carabiner fetch findyourexit/carabiner --interactive
```

You can also declare a source in `carabiner.jsonc` and run `carabiner install`. See [Declarative Skill Sources](declarative-sources.md) for the source format.
