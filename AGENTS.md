# Agent Guidelines

Code quality and organization standards for this project.

## Requirements

1. **Rust only** - No other languages in src/
2. **Unit test each module** - Tests live in the same file with `#[cfg(test)]`
3. **Conventional commits** - Commit when a reasonable amount of work is done
4. **Run checks before committing** - `cargo fmt && cargo clippy && cargo test`
5. **Keep functions focused** - Prefer 2-4 arguments, pass primitives directly
6. **Short variable names** - Prefer concise, readable names

## Code Organization

```
src/
├── api/        # GitHub API client and types
├── graph.rs    # PR dependency graph building
├── markdown.rs # Markdown table generation
├── persist.rs  # PR description updates
├── git.rs      # Git operations (rebase, cherry-pick)
├── util.rs     # Shared utilities
├── lib.rs      # Library exports
└── main.rs     # CLI entry point
```

## Testing

- Unit tests use `#[cfg(test)]` modules in each file
- Snapshot tests use `insta` for markdown output
- API mocking uses `mockito` for HTTP tests
- Run tests: `cargo test`
- Review snapshots: `cargo insta review`

## Commit Style

Use conventional commits:

```
feat: add new feature
fix: resolve bug
perf: improve performance
refactor: restructure code
test: add or update tests
docs: update documentation
chore: maintenance tasks
```

## Stacked PRs

When creating stacked PRs, use `gh-stack` to manage and annotate them:

1. Create branches that build on each other:
   ```bash
   git checkout -b feat/my-feature-part1
   # make changes, commit
   git checkout -b feat/my-feature-part2
   # make changes, commit
   ```

2. Push branches and create PRs with proper base branches:
   ```bash
   git push origin feat/my-feature-part1 feat/my-feature-part2
   gh pr create --base master --head feat/my-feature-part1 --title "[STACK-ID] part 1"
   gh pr create --base feat/my-feature-part1 --head feat/my-feature-part2 --title "[STACK-ID] part 2"
   ```

3. Annotate PRs with stack info:
   ```bash
   gh-stack annotate 'STACK-ID' -r 'luqven/gh-stack' --ci
   ```

4. After rebasing, update the stack:
   ```bash
   gh-stack autorebase 'STACK-ID' -r 'luqven/gh-stack' -C . --ci
   ```

## Function Signatures

Pass primitives directly, keep arg count low:

```rust
// Good - clear, focused
fn base_request(client: &Client, credentials: &Credentials, url: &str) -> RequestBuilder

fn build_table(deps: &FlatDep, title: &str, prelude: Option<&str>, repo: &str) -> String

async fn perform_rebase(deps: FlatDep, repo: &Repository, remote: &str, boundary: Option<&str>, ci: bool)
```

## Naming

- Short but clear: `pr` not `pull_request`, `creds` not `credentials`
- Async functions: no special suffix (Rust async is explicit)
- Boolean flags: prefix with `is_`/`has_` (e.g., `is_draft`)

## Dependencies

- `reqwest` - HTTP client
- `tokio` - Async runtime
- `serde` - Serialization
- `git2` - Git operations
- `petgraph` - Graph algorithms
- `clap` - CLI parsing
- `dialoguer` - Interactive prompts
