# gh-stack land

Merge an entire stack by squash-merging the topmost approved PR and closing the rest.

## Usage

```bash
gh-stack land 'STACK-ID'
gh-stack land 'STACK-ID' --dry-run      # preview without changes
gh-stack land 'STACK-ID' --count 2      # only land bottom 2 PRs
gh-stack land 'STACK-ID' --no-approval  # skip approval check
```

## How it works

1. Orders the stack from base to top
2. Finds the topmost PR that can be merged (approved, not draft)
3. Squash-merges that PR into its base
4. Closes all PRs below it with a comment linking to the merge

This works because each PR contains all commits from PRs below it. Squash-merging the top PR lands all changes at once.

## Flags

| Flag | Description |
|------|-------------|
| `--dry-run` | Preview what would happen without making changes |
| `--count N` | Only land the bottom N PRs in the stack |
| `--no-approval` | Skip the approval requirement check |
| `-r`, `--repository` | Override repository (owner/repo) |
| `-o`, `--origin` | Git remote name (default: origin) |
| `-e`, `--excl` | Exclude PR by number (repeatable) |

## Requirements

- PRs must be approved (unless `--no-approval`)
- Draft PRs block landing
- The PR being merged must pass branch protection rules

## Example

```
Stack before:
  #103 [STACK-ID] Part 3  (approved)
  #102 [STACK-ID] Part 2  (approved)
  #101 [STACK-ID] Part 1  (approved, base: main)

After `gh-stack land 'STACK-ID'`:
  #103 squash-merged into main
  #102 closed (comment: "Landed via #103")
  #101 closed (comment: "Landed via #103")
```

## When to use

- All PRs in the stack are approved
- CI is passing on the top PR
- Ready to merge to main/master

## See also

- [log](log.md) - Check stack status before landing
- [annotate](annotate.md) - Update PR descriptions
