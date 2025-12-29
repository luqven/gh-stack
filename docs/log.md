# gh-stack log

Visualize your stack's structure and status.

## Usage

```bash
gh-stack log 'STACK-ID'
gh-stack log 'STACK-ID' --short         # compact list view
gh-stack log 'STACK-ID' --include-closed # show closed/merged PRs
gh-stack log 'STACK-ID' --no-color      # disable colors and unicode
gh-stack log 'STACK-ID' -C /path/to/repo # specify repo path
```

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
| `--short`, `-s` | Compact list format instead of tree |
| `--include-closed` | Show branches with closed/merged PRs |
| `--no-color` | Disable colors and unicode characters |
| `-C`, `--project` | Path to local repository |
| `-r`, `--repository` | Override repository (owner/repo) |
| `-o`, `--origin` | Git remote name (default: origin) |
| `-e`, `--excl` | Exclude PR by number (repeatable) |

## When to use

- Before rebasing to understand stack structure
- To check which PRs are open, merged, or draft
- To see recent commits on each branch
- To verify stack order before landing

## See also

- [annotate](annotate.md) - Add stack tables to PR descriptions
- [land](land.md) - Merge the stack
