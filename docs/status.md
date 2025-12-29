# gh-stack status

Show stack status with CI, approval, and merge readiness indicators.

## Usage

```bash
# Infer stack from current branch (recommended)
gh-stack status

# Infer from a specific branch
gh-stack status --branch feat/my-feature

# List all stacks and select interactively
gh-stack status --all

# Search by identifier in PR titles
gh-stack status 'STACK-ID'

# CI mode (non-interactive)
gh-stack status --branch $GITHUB_HEAD_REF --ci

# Use with log command
gh-stack log --status 'STACK-ID'
```

## Stack Discovery

When run without an identifier, `gh-stack status` automatically discovers your stack by:

1. Finding the PR for your current branch
2. Walking up the PR base chain to find ancestors
3. Walking down to find PRs that build on yours

This works with any PR structure - no special naming required.

## Description

The `status` command displays your PR stack with status bits showing:

1. **CI checks** - Whether CI/GitHub Actions are passing
2. **Approved** - Whether the PR has been approved
3. **Mergeable** - Whether the PR has merge conflicts
4. **Stack clear** - Whether all PRs below are approved and not draft

## Output Format

### Default (Unicode)

```
◉ feature-3 (current) #125 - Add final feature component...
│ [✓ ✗ ✓ ✗]  2 hours ago
│
│ - abc1234 Add widget component
│ - def5678 Update styles
│ + 2 more
│
◯ feature-2 #124 - Implement API endpoint
│ [✓ ✓ ✓ ✓]  1 day ago
│
◯ feature-1 #123 - Setup base infrastructure
│ [✓ ✓ ✗ ✓]  3 days ago
│
◯ main

Status: [CI | Approved | Mergeable | Stack]
  ✓ pass  ✗ fail  ⏳ pending  ─ n/a
```

### ASCII mode (--no-color)

```
* feature-3 (current) #125 - Add final feature component...
| [Y N Y N]  2 hours ago
|
o main

Status: [CI | Approved | Mergeable | Stack]
  Y=pass  N=fail  ?=pending  -=n/a
```

## Status Bits

| Position | Meaning | Pass | Fail | Pending |
|----------|---------|------|------|---------|
| 1st | CI checks | All checks pass | Any check failed | Checks running |
| 2nd | Approved | Has approval | No approval | - |
| 3rd | Mergeable | No conflicts | Has conflicts | Computing |
| 4th | Stack clear | All below approved | Blocked by PR below | - |

## Options

| Flag | Description |
|------|-------------|
| `--branch`, `-b` | Infer stack from this branch instead of current |
| `--all`, `-a` | List all stacks and select interactively |
| `--ci` | Non-interactive mode for CI environments |
| `--trunk` | Override trunk branch (default: auto-detect or "main") |
| `--no-checks` | Skip fetching CI/approval/conflict status (faster) |
| `--no-color` | Disable colors and Unicode characters |
| `--help-legend` | Show status bits legend |
| `--json` | Output in JSON format |
| `-C, --project <PATH>` | Path to local repository |
| `-r, --repository <REPO>` | Specify repository (owner/repo) |
| `-o, --origin <REMOTE>` | Git remote to use (default: origin) |
| `-e, --excl <NUMBER>` | Exclude PR by number (repeatable) |

## CI Usage

In CI environments, use `--ci` to disable interactive prompts:

```bash
# Must provide branch explicitly
gh-stack status --branch $GITHUB_HEAD_REF --ci

# Or use identifier
gh-stack status 'STACK-ID' --ci

# JSON output for parsing
gh-stack status --branch $GITHUB_HEAD_REF --ci --json
```

The `--ci` flag will:
- Fail with an error if no identifier or branch is provided
- Fail if on a trunk branch without an identifier
- Never prompt for user input

## JSON Output

```bash
gh-stack status --json
```

```json
{
  "stack": [
    {
      "branch": "feature-3",
      "pr_number": 125,
      "title": "Add final feature component",
      "is_current": true,
      "is_draft": false,
      "status": {
        "ci": "passed",
        "approved": "failed",
        "mergeable": "passed",
        "stack_clear": "failed"
      },
      "updated_at": "2024-01-15T10:30:00Z",
      "commits": [
        {"sha": "abc1234", "message": "Add widget component"}
      ]
    }
  ],
  "trunk": "main"
}
```

## Legend

The legend is shown automatically on first run. To see it again:

```bash
gh-stack status --help-legend
```

The legend marker is stored in `~/.gh-stack-legend-seen`.

## Examples

### Infer from current branch

```bash
gh-stack status
```

### Infer from specific branch

```bash
gh-stack status --branch feat/my-feature
```

### Skip API calls for faster output

```bash
gh-stack status --no-checks
```

### Specify repository explicitly

```bash
gh-stack status -r owner/repo
```

### Output as JSON for scripting

```bash
gh-stack status --json | jq '.stack[].status.ci'
```

## See Also

- [log](log.md) - Basic tree view without status bits
- [land](land.md) - Land a stack of PRs
- [annotate](annotate.md) - Annotate PRs with stack information
