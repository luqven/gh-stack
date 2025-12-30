# gh-stack log

Visualize your stack's structure and status.

## Usage

```bash
# Infer stack from current branch (recommended)
gh-stack log

# Infer from a specific branch
gh-stack log --branch feat/my-feature

# List all stacks and select interactively
gh-stack log --all

# Search by identifier in PR titles
gh-stack log 'STACK-ID'

# CI mode (non-interactive)
gh-stack log --branch $GITHUB_HEAD_REF --ci
```

## Stack Discovery

When run without an identifier, `gh-stack log` automatically discovers your stack by:

1. Finding the PR for your current branch
2. Walking up the PR base chain to find ancestors
3. Walking down to find PRs that build on yours

This works with any PR structure - no special naming required.

## Output

The default tree view shows:

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
◯ feat/part-1 (draft)
│ 5 hours ago
│
◯ main
```

- `◉` marks the current branch
- `◯` marks other branches
- Commits are shown when run from a git repo
- Timestamps show when each PR was last updated
- Draft PRs are labeled

The `--short` flag shows a compact list:

```
#103: [STACK-ID] Add validation (Merges into #102)
#102: [STACK-ID] Implement feature (Merges into #101)
#101: [STACK-ID] Initial scaffolding (Base)
```

## Flags

| Flag | Description |
|------|-------------|
| `--branch`, `-b` | Infer stack from this branch instead of current |
| `--all`, `-a` | List all stacks and select interactively |
| `--ci` | Non-interactive mode for CI environments |
| `--trunk` | Override trunk branch (default: auto-detect or "main") |
| `--short`, `-s` | Compact list format instead of tree |
| `--status` | Show CI, approval, and merge status bits |
| `--include-closed` | Show branches with closed/merged PRs |
| `--no-color` | Disable colors and unicode characters |
| `-C`, `--project` | Path to local repository |
| `-r`, `--repository` | Override repository (owner/repo) |
| `-o`, `--origin` | Git remote name (default: origin) |
| `-e`, `--excl` | Exclude PR by number (repeatable) |

## CI Usage

In CI environments, use `--ci` to disable interactive prompts:

```bash
# Must provide branch explicitly
gh-stack log --branch $GITHUB_HEAD_REF --ci

# Or use identifier
gh-stack log 'STACK-ID' --ci
```

The `--ci` flag will:
- Fail with an error if no identifier or branch is provided
- Fail if on a trunk branch without an identifier
- Never prompt for user input

## On Trunk Branch

When you're on a trunk branch (main, master, etc.), `gh-stack log` will prompt you to:

1. Enter a stack identifier manually
2. Select from detected stacks in the repository
3. Cancel

```
You're on 'main' (trunk branch).

? What would you like to do?
> Enter a stack identifier
  Select from detected stacks (3 found)
  Cancel
```

## No PR Found

If no PR exists for your current branch:

```
No PR found for branch 'feat/new-feature'.

Create a PR with:
  gh pr create --base main --head feat/new-feature

? Create PR now? [Y/n]
```

If you confirm, `gh-stack` will run `gh pr create` for you.

## When to use

- Before rebasing to understand stack structure
- To check which PRs are open, merged, or draft
- To see recent commits on each branch
- To verify stack order before landing

## See also

- [annotate](annotate.md) - Add stack tables to PR descriptions
- [status](status.md) - Show CI and approval status
- [land](land.md) - Merge the stack
