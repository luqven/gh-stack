# gh-stack rebase

Generate a bash script to manually rebase a stack.

## Usage

```bash
gh-stack rebase 'STACK-ID' > rebase.sh
chmod +x rebase.sh
./rebase.sh
```

## How it works

Outputs a bash script with git commands to:

1. Check out each branch in order
2. Rebase onto the previous branch
3. Handle the stack reconstruction step-by-step

This gives you full control over the rebase process.

## Flags

| Flag | Description |
|------|-------------|
| `-e`, `--excl` | Exclude PR by number (repeatable) |

## When to use

- When `autorebase` doesn't fit your workflow
- When you need to inspect/modify the rebase steps
- For debugging stack issues
- When you want to dry-run before executing

## Warnings

- The generated script may need manual adjustments
- Review the script before executing
- Back up your work first

## See also

- [autorebase](autorebase.md) - Automatic stack rebuilding
- [log](log.md) - Verify stack structure
