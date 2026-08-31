# Command Syntax

Carabiner reads custom command sources from `.carabiner/commands/*.md`. A source file has optional YAML frontmatter followed by a Markdown prompt body. Run `carabiner generate --targets <target> --features commands` to generate commands for a selected target.

## Source file

Save the following file as `.carabiner/commands/summarize.md`:

```md title=".carabiner/commands/summarize.md"
---
targets: ["claudecode"]
description: "Summarize git diff"
---

Summarize the diff:

!`git diff`

Focus on $ARGUMENTS.
```

`targets` is optional. It accepts target identifiers such as `claudecode` or the wildcard `*`. When it is omitted, Carabiner considers the command for every selected target that supports commands. `description` is canonical command metadata. Add target-specific metadata below that target's frontmatter key when the target supports extra command settings.

For the `claudecode` target, this example generates `.claude/commands/summarize.md`. The source filename supplies the command filename unless the selected target uses a different naming convention.

## Placeholder handling

The prompt body is not a Carabiner template. Carabiner does not expand `$ARGUMENTS`, execute `` !`cmd` ``, or translate placeholder syntax between targets. It retains those tokens as command-body text while writing the target's native command format.

| Target | `$ARGUMENTS` | `` !`cmd` `` |
| --- | --- | --- |
| Claude Code (`claudecode`) | Claude Code expands this to the full argument string. | Claude Code runs `cmd` and substitutes its output. |
| Codex CLI (`codexcli`) | Passed to the model as literal prompt text. | Passed to the model as literal prompt text. |
| Pi (`pi`) | Pi expands this token. Pi also supports `$1`, `$2`, and `$@`. | Carabiner leaves this token in place. Do not assume Pi expands inline shell snippets. |
| Other command targets | Carabiner passes the token through. Refer to the target's documentation for its behavior. | Carabiner passes the token through. Refer to the target's documentation for its behavior. |

The example above uses Claude Code syntax. After running the following command, Claude Code interprets the placeholders when the generated command is invoked:

```bash
carabiner generate --targets claudecode --features commands
```

Carabiner itself never executes the command body.

## Portability

Use only syntax understood by every target selected in `targets`. A command that relies on Claude Code placeholder behavior is not portable to a target that treats those tokens as prompt text. If a command needs target-specific argument behavior, limit its `targets` list to the targets that support that syntax.

## Importing commands

`carabiner import --targets <target> --features commands` writes imported commands to `.carabiner/commands/*.md`. Import converts supported frontmatter fields into canonical or target-specific metadata, but it does not reverse-translate command-body placeholders. A placeholder in an imported command remains written in the form used by that command.

Text in fenced code blocks and inline code receives the same treatment. Carabiner does not interpret placeholder tokens in any part of the command body.
