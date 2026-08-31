# Official Skills

Carabiner can fetch skills from GitHub repositories into a project's `.carabiner/` directory. The official collection is published with the Carabiner project.

## Fetch the Collection

```bash
carabiner fetch findyourexit/carabiner
```

By default, `fetch` requests the `skills` feature and writes fetched files to `.carabiner/`.

## Choose Skills Interactively

Use an interactive terminal to choose individual skills from the source.

```bash
carabiner fetch findyourexit/carabiner --interactive
```

You can also declare a source in `carabiner.jsonc` and run `carabiner install`. See [Declarative Skill Sources](/guide/declarative-sources) for the source format.
