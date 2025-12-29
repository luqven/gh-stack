# Troubleshooting

Common issues and solutions.

## Authentication

### "Bad credentials" error

Your GitHub token is invalid or expired.

```bash
# Check your token
echo $GHSTACK_OAUTH_TOKEN

# Generate a new token at https://github.com/settings/tokens
# Required scope: repo
export GHSTACK_OAUTH_TOKEN='<new token>'
```

### "You didn't pass GHSTACK_OAUTH_TOKEN"

The environment variable isn't set.

```bash
export GHSTACK_OAUTH_TOKEN='<your token>'

# Or add to .gh-stack.env in your project root
echo "GHSTACK_OAUTH_TOKEN=<your token>" > .gh-stack.env
```

## Stack Detection

### "No PRs found matching 'IDENTIFIER'"

- Verify the identifier exists in PR titles
- Check you're searching the right repository
- Use `-r owner/repo` to specify the repository explicitly

```bash
# Debug: search manually
gh pr list --search 'IDENTIFIER in:title'
```

### PRs from wrong repository

The identifier matched PRs in multiple repositories.

```bash
# Specify repository explicitly
gh-stack log 'STACK-ID' -r 'owner/repo'

# Or set the environment variable
export GHSTACK_TARGET_REPOSITORY='owner/repo'
```

### Stack order is wrong

PRs must have their base branch set correctly:

- Bottom PR: base is `main` (or your default branch)
- Each PR above: base is the branch below it

## Autorebase

### Conflicts during cherry-pick

```bash
# 1. Resolve conflicts in your editor
# 2. Stage resolved files
git add <resolved files>

# 3. Continue cherry-picking
git cherry-pick --continue
```

### "The --project argument is required"

```bash
gh-stack autorebase 'STACK-ID' -C /path/to/repo
# Or from the repo directory:
gh-stack autorebase 'STACK-ID' -C .
```

### Local branches out of sync after autorebase

```bash
# Fetch latest from remote
git fetch origin

# Reset local branch to remote
git checkout <branch>
git reset --hard origin/<branch>
```

## Land

### "PR #X requires approval"

Get approval on the PR, or skip the check:

```bash
gh-stack land 'STACK-ID' --no-approval
```

### "PR #X is a draft and blocks landing"

Mark the PR as ready for review on GitHub, then retry.

### Merge failed

Check branch protection rules on the repository. The PR may need:

- Passing CI checks
- Required reviewers
- Up-to-date branch

## Log

### Tree view shows only trunk branch

Closed/merged PRs are hidden by default.

```bash
gh-stack log 'STACK-ID' --include-closed
```

### No commits shown in tree view

Run from inside a git repository, or specify the path:

```bash
gh-stack log 'STACK-ID' -C /path/to/repo
```

## Still stuck?

Open an issue: https://github.com/luqven/gh-stack/issues
