# gh-stack autorebase

Rebuild and push an entire stack after local changes.

## Usage

```bash
gh-stack autorebase 'STACK-ID' -C /path/to/repo
gh-stack autorebase 'STACK-ID' -C /path/to/repo --ci  # skip confirmation
gh-stack autorebase 'STACK-ID' -C /path/to/repo -b <sha>  # cherry-pick boundary
```

## How it works

1. Checks out the base branch (e.g., `main`)
2. Cherry-picks commits from each PR in stack order
3. Updates local branches to point at new commits
4. Force-pushes all branches at once

This reconstructs a clean, linear stack from your local changes.

## Flags

| Flag | Description |
|------|-------------|
| `-C`, `--project` | Path to local repository (required) |
| `--ci` | Skip confirmation prompt |
| `-b`, `--initial-cherry-pick-boundary` | Stop initial cherry-pick at this SHA |
| `-o`, `--origin` | Git remote name (default: origin) |
| `-r`, `--repository` | Override repository (owner/repo) |
| `-e`, `--excl` | Exclude PR by number (repeatable) |

## Conflict handling

If a conflict occurs during cherry-picking:

1. The process pauses
2. Resolve conflicts manually
3. Stage resolved files with `git add`
4. Continue with `git cherry-pick --continue`

## Example

After amending a commit in the middle of your stack:

```bash
# Your local history diverged from remote
git checkout feat/part-1
git commit --amend -m "Updated message"

# Rebuild and sync the entire stack
gh-stack autorebase 'STACK-ID' -C .
```

## Warnings

- **Force-pushes** to all branches in the stack
- Back up your work before running
- Collaborators will need to reset their local branches

## When to use

- After amending commits
- After interactive rebase
- After resolving conflicts with upstream
- After reordering commits

## See also

- [log](log.md) - Verify stack structure after rebase
- [rebase](rebase.md) - Generate a rebase script for manual control
