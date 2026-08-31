# Why Carabiner?

## Keep Sources Together

Store shared rules, skills, commands, and related configuration in `.carabiner/`. This gives a team one place to maintain guidance for its AI coding tools.

## Generate Native Configuration

Carabiner translates the same source files into the native configuration formats used by selected tools. Developers can use different supported tools without maintaining separate copies of the same guidance.

## Review the Output

Generated files are ordinary configuration files. Teams can review them, commit them, and use `--check` to confirm that they match the source files.

## Use Project or Global Scope

Use project mode for repository-specific guidance. Use global mode for personal settings that apply across projects when the selected target supports global configuration.

## Change Tools Without Rewriting Rules

When a team adds or removes a supported AI coding tool, it can update the selected targets and generate the required configuration again. The source rules remain in one place.
