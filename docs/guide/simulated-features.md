# Simulated Commands, Subagents, and Skills

Simulation flags add instructions to a generated rules file that describe source commands, subagents, and skills as simulated features. This is useful when the selected target reads the generated rules but does not provide the same native feature format.

This guide uses `agentsmd`, whose commands, subagents, and skills are generated as simulated features in project mode.

## Prepare the Source Files

Create source files in the usual `.carabiner/` layout.

- `.carabiner/commands/review.md`
- `.carabiner/subagents/planner.md`
- `.carabiner/skills/project-context/SKILL.md`

## Generate the Conventions

Generate rules and the simulated features with the relevant flags.

```bash
carabiner generate \
  --targets agentsmd \
  --features rules,commands,subagents,skills \
  --simulate-commands \
  --simulate-subagents \
  --simulate-skills
```

The generated `AGENTS.md` tells an assistant how to locate the source files. For example, `s/review` refers to `.carabiner/commands/review.md`, and a request to call the `planner` subagent refers to `.carabiner/subagents/planner.md`.

Use simulated feature syntax only in prompts interpreted by the AI coding tool. It is not a shell command.
