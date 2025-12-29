# gh-stack

[![CI](https://github.com/luqven/gh-stack/actions/workflows/ci.yml/badge.svg)](https://github.com/luqven/gh-stack/actions/workflows/ci.yml)

Manage stacked pull requests on GitHub.

## Features

- **Visualize** your stack with `gh-stack log`
- **Annotate** PR descriptions with stack metadata tables
- **Rebase** entire stacks after local changes
- **Land** a stack by squash-merging the top PR and closing the rest

## Installation

```bash
brew tap luqven/gh-stack
brew install gh-stack
```

<details>
<summary>Build from source</summary>

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo install --force --path .
```
</details>

## Setup

```bash
export GHSTACK_OAUTH_TOKEN='<personal access token>'  # repo scope required
# Optional: override auto-detected repository
export GHSTACK_TARGET_REPOSITORY='owner/repo'
```

You can also set these in a `.gh-stack.env` file.

## Commands

### log

Visualize your stack. [Learn more](docs/log.md)

```bash
gh-stack log 'STACK-ID'
gh-stack log 'STACK-ID' --short  # compact list view
```

<details>
<summary>Example output</summary>

```
◉ feat/part-3 (current)
│ 2 hours ago
│
│ a1b2c3d - Add validation logic
│ f4e5d6c - Update tests
│
◯ feat/part-2
│ 3 hours ago
│
│ 1a2b3c4 - Implement core feature
│
◯ feat/part-1
│ 5 hours ago
│
│ 9z8y7x6 - Initial scaffolding
│
◯ main
```
</details>

### annotate

Add a markdown table to each PR description. [Learn more](docs/annotate.md)

```bash
gh-stack annotate 'STACK-ID'
gh-stack annotate 'STACK-ID' --badges  # shields.io badges (public repos)
gh-stack annotate 'STACK-ID' --ci      # skip confirmation
```

<details>
<summary>Example output</summary>

This adds a table to each PR description:

```markdown
### Stack: STACK-ID

| PR | Title | Base |
|:--:|:------|:----:|
| #103 | [STACK-ID] Add validation | #102 |
| #102 | [STACK-ID] Implement feature | #101 |
| #101 | [STACK-ID] Initial scaffolding | main |
```

GitHub auto-links PR numbers, showing status on hover.
</details>

### land

Squash-merge the topmost approved PR and close the rest. [Learn more](docs/land.md)

```bash
gh-stack land 'STACK-ID'
gh-stack land 'STACK-ID' --dry-run      # preview changes
gh-stack land 'STACK-ID' --count 2      # only land bottom 2 PRs
gh-stack land 'STACK-ID' --no-approval  # skip approval check
```

### autorebase

Rebuild and push a stack after local changes. [Learn more](docs/autorebase.md)

```bash
gh-stack autorebase 'STACK-ID' -C /path/to/repo
gh-stack autorebase 'STACK-ID' -C /path/to/repo --ci  # skip confirmation
```

### rebase

Generate a bash script for manual rebasing. [Learn more](docs/rebase.md)

```bash
gh-stack rebase 'STACK-ID' > rebase.sh
```

## Workflow

1. Create branches that build on each other
2. Push and create PRs with a shared identifier (e.g., `[TICKET-123]`)
3. Set each PR's base to the branch below it
4. Use `gh-stack annotate` to add stack tables
5. After rebasing, use `gh-stack autorebase` to sync
6. Use `gh-stack land` when ready to merge

## Requirements

- All PRs in a stack share a unique identifier in their title
- All PRs live in a single GitHub repository
- Remote branches have matching local branch names

## Troubleshooting

See [docs/troubleshooting.md](docs/troubleshooting.md) for common issues and solutions.

## Disclaimer

Use at your own risk. The `autorebase` command modifies git history and force-pushes.

## Contributing

See [AGENTS.md](AGENTS.md) for coding guidelines.

## Releasing

```bash
# Update version in Cargo.toml
git commit -m "chore: release vX.Y.Z"
git tag vX.Y.Z
git push origin master vX.Y.Z
```

---

Originally created by [@timothyandrew](https://github.com/timothyandrew/gh-stack). See his [blog post on stacked PRs](https://0xc0d1.com/blog/git-stack/).
