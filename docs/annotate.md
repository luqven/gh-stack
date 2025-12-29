# gh-stack annotate

Add a markdown table to each PR description showing the full stack.

## Usage

```bash
gh-stack annotate 'STACK-ID'
gh-stack annotate 'STACK-ID' --badges    # shields.io badges (public repos)
gh-stack annotate 'STACK-ID' --ci        # skip confirmation prompt
gh-stack annotate 'STACK-ID' --prefix '#' # remove prefix from titles
gh-stack annotate 'STACK-ID' -p file.md  # prepend file contents
```

## Output

Each PR in the stack gets a table added to its description:

```markdown
### Stack: STACK-ID

| PR | Title | Base |
|:--:|:------|:----:|
| #103 | [STACK-ID] Add validation | #102 |
| #102 | [STACK-ID] Implement feature | #101 |
| #101 | [STACK-ID] Initial scaffolding | main |
```

GitHub auto-links PR numbers. Hovering shows PR status.

## Flags

| Flag | Description |
|------|-------------|
| `--badges` | Use shields.io status badges (public repos only) |
| `--ci` | Skip confirmation prompt |
| `--prefix` | Characters to strip from PR titles in the table |
| `-p`, `--prelude` | File to prepend before the table |
| `-r`, `--repository` | Override repository (owner/repo) |
| `-o`, `--origin` | Git remote name (default: origin) |
| `-e`, `--excl` | Exclude PR by number (repeatable) |

## How it works

1. Finds all PRs with the identifier in their title
2. Builds a dependency graph from PR base branches
3. Generates a markdown table
4. Updates each PR description (idempotent)

The annotation is idempotent - running it multiple times updates the existing table rather than adding duplicates.

## When to use

- After creating all PRs in a stack
- After adding or removing PRs from a stack
- After PR status changes (to update badges)

## See also

- [log](log.md) - Visualize the stack
- [land](land.md) - Merge the stack
