//! Stack discovery via GitHub API
//!
//! Walks PR base/head chains to discover full stack structure without
//! requiring identifier patterns in PR titles.
//!
//! ## Performance
//!
//! Stack discovery uses a batch-fetch strategy: all open PRs are fetched
//! in a single paginated API call, then the chain is walked in-memory.
//! This reduces API calls from O(N) to O(1) for most repositories.

use crate::api::{github_api_base, PullRequest};
use crate::Credentials;
use reqwest::Client;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::time::Duration;

/// Maximum number of pages to fetch (100 PRs per page = 1000 PRs max)
const MAX_PAGES: u32 = 10;

/// Build a GET request with auth headers
fn build_request(client: &Client, creds: &Credentials, url: &str) -> reqwest::RequestBuilder {
    client
        .get(url)
        .timeout(Duration::from_secs(10))
        .header("Authorization", format!("token {}", creds.token))
        .header("User-Agent", "luqven/gh-stack")
        .header("Accept", "application/vnd.github.v3+json")
}

/// Index of PRs for fast lookup by head/base branch.
///
/// Built once from a batch fetch, then used for in-memory chain walking.
struct PrIndex {
    by_head: HashMap<String, PullRequest>,
    by_base: HashMap<String, Vec<PullRequest>>,
}

impl PrIndex {
    /// Build an index from a list of PRs
    fn from_prs(prs: Vec<PullRequest>) -> Self {
        let mut by_head = HashMap::new();
        let mut by_base: HashMap<String, Vec<PullRequest>> = HashMap::new();

        for pr in prs {
            by_base
                .entry(pr.base().to_string())
                .or_default()
                .push(pr.clone());
            by_head.insert(pr.head().to_string(), pr);
        }

        Self { by_head, by_base }
    }

    /// Get a PR by its head branch name
    fn get_by_head(&self, head: &str) -> Option<&PullRequest> {
        self.by_head.get(head)
    }

    /// Get all PRs that target a given base branch
    fn get_by_base(&self, base: &str) -> Vec<&PullRequest> {
        self.by_base
            .get(base)
            .map(|v| v.iter().collect())
            .unwrap_or_default()
    }
}

/// Fetch a PR by its head branch name.
/// Returns None if no open PR exists for this branch.
///
/// # Arguments
/// * `repo` - Repository in "owner/repo" format
/// * `branch` - The head branch name to search for
/// * `creds` - GitHub credentials
pub async fn fetch_pr_by_head(
    repo: &str,
    branch: &str,
    creds: &Credentials,
) -> Result<Option<PullRequest>, Box<dyn Error>> {
    let client = Client::new();

    // Extract owner from repo for the head filter
    let owner = repo.split('/').next().unwrap_or(repo);
    let head_filter = format!("{}:{}", owner, branch);

    let url = format!(
        "{}/repos/{}/pulls?state=open&head={}",
        github_api_base(),
        repo,
        head_filter
    );

    let response = build_request(&client, creds, &url).send().await?;

    if response.status() == 429 {
        return Err("GitHub API rate limit exceeded".into());
    }

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(format!("Failed to fetch PR by head ({}): {}", status, text).into());
    }

    let prs: Vec<PullRequest> = response.json().await?;
    Ok(prs.into_iter().next())
}

/// Fetch all open PRs that target a given base branch.
///
/// # Arguments
/// * `repo` - Repository in "owner/repo" format
/// * `base` - The base branch name to search for
/// * `creds` - GitHub credentials
pub async fn fetch_prs_by_base(
    repo: &str,
    base: &str,
    creds: &Credentials,
) -> Result<Vec<PullRequest>, Box<dyn Error>> {
    let client = Client::new();

    let url = format!(
        "{}/repos/{}/pulls?state=open&base={}",
        github_api_base(),
        repo,
        base
    );

    let response = build_request(&client, creds, &url).send().await?;

    if response.status() == 429 {
        return Err("GitHub API rate limit exceeded".into());
    }

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(format!("Failed to fetch PRs by base ({}): {}", status, text).into());
    }

    let prs: Vec<PullRequest> = response.json().await?;
    Ok(prs)
}

/// Fetch all open PRs in a repository with pagination support.
///
/// Fetches up to MAX_PAGES pages (1000 PRs) to support enterprise users
/// with large numbers of open PRs.
///
/// # Arguments
/// * `repo` - Repository in "owner/repo" format
/// * `creds` - GitHub credentials
pub async fn fetch_all_open_prs(
    repo: &str,
    creds: &Credentials,
) -> Result<Vec<PullRequest>, Box<dyn Error>> {
    let client = Client::new();
    let mut all_prs = Vec::new();

    for page in 1..=MAX_PAGES {
        let url = format!(
            "{}/repos/{}/pulls?state=open&per_page=100&page={}",
            github_api_base(),
            repo,
            page
        );

        let response = build_request(&client, creds, &url).send().await?;

        if response.status() == 429 {
            return Err("GitHub API rate limit exceeded".into());
        }

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(format!("Failed to fetch open PRs ({}): {}", status, text).into());
        }

        let prs: Vec<PullRequest> = response.json().await?;
        let count = prs.len();
        all_prs.extend(prs);

        // GitHub returns fewer items when we've reached the end
        if count < 100 {
            break;
        }
    }

    Ok(all_prs)
}

/// Discover the full stack by walking PR chain from a starting PR.
///
/// Uses batch-fetch strategy: fetches all open PRs in one paginated call,
/// then walks the chain in-memory. This reduces API calls from O(N) to O(1).
///
/// # Arguments
/// * `repo` - Repository in "owner/repo" format
/// * `starting_pr` - The PR to start discovery from
/// * `trunk` - The trunk branch name (e.g., "main", "master")
/// * `creds` - GitHub credentials
///
/// # Returns
/// Vector of PRs in the stack, sorted from bottom (closest to trunk) to top
pub async fn discover_stack(
    repo: &str,
    starting_pr: PullRequest,
    trunk: &str,
    creds: &Credentials,
) -> Result<Vec<PullRequest>, Box<dyn Error>> {
    // Batch fetch all open PRs (1 paginated API call)
    let all_prs = fetch_all_open_prs(repo, creds).await?;

    // Build in-memory index
    let index = PrIndex::from_prs(all_prs);

    // Walk chain in memory (no more API calls)
    Ok(discover_stack_from_index(&index, starting_pr, trunk))
}

/// Walk stack using pre-fetched PR index (pure in-memory operation).
///
/// This is the core algorithm that walks up and down the PR chain
/// without making any API calls.
fn discover_stack_from_index(
    index: &PrIndex,
    starting_pr: PullRequest,
    trunk: &str,
) -> Vec<PullRequest> {
    let mut visited: HashMap<String, PullRequest> = HashMap::new();
    visited.insert(starting_pr.head().to_string(), starting_pr.clone());

    // Walk UP: follow base branches until we hit trunk
    let mut up_queue = vec![starting_pr.base().to_string()];
    let mut seen_bases: HashSet<String> = HashSet::new();

    while let Some(base) = up_queue.pop() {
        // Skip if we've seen this base or it's trunk
        if base == trunk || seen_bases.contains(&base) || visited.contains_key(&base) {
            continue;
        }
        seen_bases.insert(base.clone());

        // Try to find a PR with this branch as its head (in-memory lookup)
        if let Some(pr) = index.get_by_head(&base) {
            let pr_base = pr.base().to_string();
            visited.insert(pr.head().to_string(), pr.clone());
            up_queue.push(pr_base);
        }
    }

    // Walk DOWN: find PRs whose base is in our visited set
    let mut down_queue: Vec<String> = visited.keys().cloned().collect();
    let mut seen_heads: HashSet<String> = HashSet::new();

    while let Some(head) = down_queue.pop() {
        if seen_heads.contains(&head) {
            continue;
        }
        seen_heads.insert(head.clone());

        // Find all PRs that target this branch as their base (in-memory lookup)
        for child in index.get_by_base(&head) {
            if !visited.contains_key(child.head()) {
                let child_head = child.head().to_string();
                visited.insert(child_head.clone(), child.clone());
                down_queue.push(child_head);
            }
        }
    }

    // Sort PRs by their position in the stack (bottom to top)
    sort_stack(visited.into_values().collect(), trunk)
}

/// Sort PRs by their position in the stack (bottom to top).
/// Bottom = PR whose base is trunk, Top = PR with no children.
fn sort_stack(prs: Vec<PullRequest>, trunk: &str) -> Vec<PullRequest> {
    if prs.is_empty() {
        return prs;
    }

    // Build a map from base -> PR for sorting
    let head_to_pr: HashMap<&str, &PullRequest> = prs.iter().map(|pr| (pr.head(), pr)).collect();

    let mut sorted = Vec::with_capacity(prs.len());
    let mut remaining: HashSet<&str> = prs.iter().map(|pr| pr.head()).collect();

    // Find the root(s) - PRs whose base is trunk or not in our set
    let mut current_base = trunk;

    while !remaining.is_empty() {
        // Find a PR whose base matches current_base
        let next_pr = prs
            .iter()
            .find(|pr| remaining.contains(pr.head()) && pr.base() == current_base);

        match next_pr {
            Some(pr) => {
                remaining.remove(pr.head());
                current_base = pr.head();
                sorted.push(pr.clone());
            }
            None => {
                // No more PRs with expected base, try to find any remaining PR
                // whose base is already in sorted list or is trunk
                let sorted_heads: HashSet<&str> = sorted.iter().map(|pr| pr.head()).collect();
                let fallback = prs.iter().find(|pr| {
                    remaining.contains(pr.head())
                        && (pr.base() == trunk || sorted_heads.contains(pr.base()))
                });

                match fallback {
                    Some(pr) => {
                        remaining.remove(pr.head());
                        current_base = pr.head();
                        sorted.push(pr.clone());
                    }
                    None => {
                        // Add any remaining PRs (shouldn't happen in well-formed stacks)
                        for head in remaining.iter() {
                            if let Some(pr) = head_to_pr.get(head) {
                                sorted.push((*pr).clone());
                            }
                        }
                        break;
                    }
                }
            }
        }
    }

    sorted
}

/// Discover all stacks in a repository.
///
/// Groups PRs by their root (PR whose base is trunk) and returns
/// each group as a separate stack. Uses batch-fetch for efficiency.
///
/// # Arguments
/// * `repo` - Repository in "owner/repo" format
/// * `trunk` - The trunk branch name (e.g., "main", "master")
/// * `creds` - GitHub credentials
///
/// # Returns
/// Vector of stacks, where each stack is a vector of PRs sorted bottom to top
pub async fn discover_all_stacks(
    repo: &str,
    trunk: &str,
    creds: &Credentials,
) -> Result<Vec<Vec<PullRequest>>, Box<dyn Error>> {
    let all_prs = fetch_all_open_prs(repo, creds).await?;
    Ok(group_into_stacks(all_prs, trunk))
}

/// Group PRs into stacks (pure in-memory operation).
///
/// PRs are grouped by walking from each root (PR whose base is trunk)
/// down through child PRs.
fn group_into_stacks(prs: Vec<PullRequest>, trunk: &str) -> Vec<Vec<PullRequest>> {
    if prs.is_empty() {
        return vec![];
    }

    // Build adjacency: base -> list of PRs targeting that base
    let mut base_to_prs: HashMap<String, Vec<&PullRequest>> = HashMap::new();
    for pr in &prs {
        base_to_prs
            .entry(pr.base().to_string())
            .or_default()
            .push(pr);
    }

    // Find root PRs (those whose base is trunk)
    let roots: Vec<&PullRequest> = prs.iter().filter(|pr| pr.base() == trunk).collect();

    // For each root, build its stack by walking down
    let mut stacks = Vec::new();
    let mut assigned: HashSet<usize> = HashSet::new();

    for root in roots {
        if assigned.contains(&root.number()) {
            continue;
        }

        let mut stack = vec![(*root).clone()];
        assigned.insert(root.number());

        // BFS to find all descendants
        let mut queue = vec![root.head()];
        while let Some(head) = queue.pop() {
            if let Some(children) = base_to_prs.get(head) {
                for child in children {
                    if !assigned.contains(&child.number()) {
                        assigned.insert(child.number());
                        stack.push((*child).clone());
                        queue.push(child.head());
                    }
                }
            }
        }

        // Sort the stack
        stack = sort_stack(stack, trunk);
        stacks.push(stack);
    }

    // Sort stacks by size (largest first) for better UX
    stacks.sort_by_key(|s| std::cmp::Reverse(s.len()));

    stacks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::PullRequestStatus;
    use mockito::Server;
    use serial_test::serial;

    fn make_pr_json(number: usize, head: &str, base: &str, title: &str) -> String {
        format!(
            r#"{{
                "id": {number},
                "number": {number},
                "head": {{"label": "user:{head}", "ref": "{head}", "sha": "abc{number}"}},
                "base": {{"label": "user:{base}", "ref": "{base}", "sha": "def{number}"}},
                "title": "{title}",
                "url": "https://api.github.com/repos/test/repo/pulls/{number}",
                "body": null,
                "state": "open",
                "merged_at": null,
                "updated_at": null,
                "draft": false
            }}"#
        )
    }

    fn make_test_pr(number: usize, head: &str, base: &str) -> PullRequest {
        PullRequest::new_for_test(
            number,
            head,
            base,
            &format!("PR {}", number),
            PullRequestStatus::Open,
            false,
            None,
            vec![],
        )
    }

    // === PrIndex tests ===

    #[test]
    fn test_pr_index_get_by_head() {
        let pr1 = make_test_pr(1, "feature-1", "main");
        let pr2 = make_test_pr(2, "feature-2", "feature-1");

        let index = PrIndex::from_prs(vec![pr1, pr2]);

        assert!(index.get_by_head("feature-1").is_some());
        assert_eq!(index.get_by_head("feature-1").unwrap().number(), 1);
        assert!(index.get_by_head("feature-2").is_some());
        assert!(index.get_by_head("nonexistent").is_none());
    }

    #[test]
    fn test_pr_index_get_by_base() {
        let pr1 = make_test_pr(1, "feature-1", "main");
        let pr2 = make_test_pr(2, "feature-2", "main");
        let pr3 = make_test_pr(3, "feature-3", "feature-1");

        let index = PrIndex::from_prs(vec![pr1, pr2, pr3]);

        let main_children = index.get_by_base("main");
        assert_eq!(main_children.len(), 2);

        let feature1_children = index.get_by_base("feature-1");
        assert_eq!(feature1_children.len(), 1);
        assert_eq!(feature1_children[0].number(), 3);

        let no_children = index.get_by_base("feature-3");
        assert!(no_children.is_empty());
    }

    // === discover_stack_from_index tests ===

    #[test]
    fn test_discover_stack_from_index_linear() {
        let pr1 = make_test_pr(1, "feature-1", "main");
        let pr2 = make_test_pr(2, "feature-2", "feature-1");
        let pr3 = make_test_pr(3, "feature-3", "feature-2");

        let index = PrIndex::from_prs(vec![pr1.clone(), pr2, pr3]);

        // Start from middle of stack
        let stack = discover_stack_from_index(&index, pr1, "main");

        assert_eq!(stack.len(), 3);
        assert_eq!(stack[0].number(), 1); // bottom
        assert_eq!(stack[1].number(), 2);
        assert_eq!(stack[2].number(), 3); // top
    }

    #[test]
    fn test_discover_stack_from_index_from_top() {
        let pr1 = make_test_pr(1, "feature-1", "main");
        let pr2 = make_test_pr(2, "feature-2", "feature-1");
        let pr3 = make_test_pr(3, "feature-3", "feature-2");

        let index = PrIndex::from_prs(vec![pr1, pr2, pr3.clone()]);

        // Start from top of stack
        let stack = discover_stack_from_index(&index, pr3, "main");

        assert_eq!(stack.len(), 3);
        assert_eq!(stack[0].number(), 1); // bottom
        assert_eq!(stack[2].number(), 3); // top
    }

    #[test]
    fn test_discover_stack_from_index_single_pr() {
        let pr = make_test_pr(1, "feature", "main");
        let index = PrIndex::from_prs(vec![pr.clone()]);

        let stack = discover_stack_from_index(&index, pr, "main");

        assert_eq!(stack.len(), 1);
        assert_eq!(stack[0].number(), 1);
    }

    #[test]
    fn test_discover_stack_from_index_unrelated_prs() {
        // Two separate stacks - should only discover one
        let pr1 = make_test_pr(1, "feature-1", "main");
        let pr2 = make_test_pr(2, "other-stack", "main");

        let index = PrIndex::from_prs(vec![pr1.clone(), pr2]);

        let stack = discover_stack_from_index(&index, pr1, "main");

        assert_eq!(stack.len(), 1);
        assert_eq!(stack[0].number(), 1);
    }

    // === group_into_stacks tests ===

    #[test]
    fn test_group_into_stacks_single_stack() {
        let pr1 = make_test_pr(1, "feature-1", "main");
        let pr2 = make_test_pr(2, "feature-2", "feature-1");

        let stacks = group_into_stacks(vec![pr1, pr2], "main");

        assert_eq!(stacks.len(), 1);
        assert_eq!(stacks[0].len(), 2);
    }

    #[test]
    fn test_group_into_stacks_multiple_stacks() {
        let pr1 = make_test_pr(1, "feature-1", "main");
        let pr2 = make_test_pr(2, "feature-2", "feature-1");
        let pr3 = make_test_pr(3, "other-1", "main");

        let stacks = group_into_stacks(vec![pr1, pr2, pr3], "main");

        assert_eq!(stacks.len(), 2);
        // Larger stack first
        assert_eq!(stacks[0].len(), 2);
        assert_eq!(stacks[1].len(), 1);
    }

    #[test]
    fn test_group_into_stacks_empty() {
        let stacks = group_into_stacks(vec![], "main");
        assert!(stacks.is_empty());
    }

    // === sort_stack tests ===

    #[test]
    fn test_sort_stack_linear() {
        let pr1 = make_test_pr(1, "feature-1", "main");
        let pr2 = make_test_pr(2, "feature-2", "feature-1");
        let pr3 = make_test_pr(3, "feature-3", "feature-2");

        // Give them in wrong order
        let prs = vec![pr3, pr1, pr2];
        let sorted = sort_stack(prs, "main");

        assert_eq!(sorted.len(), 3);
        assert_eq!(sorted[0].number(), 1); // base: main
        assert_eq!(sorted[1].number(), 2); // base: feature-1
        assert_eq!(sorted[2].number(), 3); // base: feature-2
    }

    #[test]
    fn test_sort_stack_single() {
        let pr = make_test_pr(1, "feature", "main");

        let sorted = sort_stack(vec![pr], "main");
        assert_eq!(sorted.len(), 1);
        assert_eq!(sorted[0].number(), 1);
    }

    #[test]
    fn test_sort_stack_empty() {
        let sorted = sort_stack(vec![], "main");
        assert!(sorted.is_empty());
    }

    // === API tests with mocks ===

    #[tokio::test]
    #[serial]
    async fn test_fetch_pr_by_head_found() {
        let mut server = Server::new_async().await;

        let pr_json = make_pr_json(42, "feature-branch", "main", "Test PR");

        let mock = server
            .mock("GET", "/repos/owner/repo/pulls")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("state".into(), "open".into()),
                mockito::Matcher::UrlEncoded("head".into(), "owner:feature-branch".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(format!("[{}]", pr_json))
            .create_async()
            .await;

        std::env::set_var("GITHUB_API_BASE", server.url());

        let creds = Credentials::new("test-token");
        let result = fetch_pr_by_head("owner/repo", "feature-branch", &creds).await;

        assert!(result.is_ok());
        let pr = result.unwrap();
        assert!(pr.is_some());
        assert_eq!(pr.unwrap().number(), 42);

        mock.assert_async().await;
    }

    #[tokio::test]
    #[serial]
    async fn test_fetch_pr_by_head_not_found() {
        let mut server = Server::new_async().await;

        let mock = server
            .mock("GET", "/repos/owner/repo/pulls")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("state".into(), "open".into()),
                mockito::Matcher::UrlEncoded("head".into(), "owner:nonexistent".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("[]")
            .create_async()
            .await;

        std::env::set_var("GITHUB_API_BASE", server.url());

        let creds = Credentials::new("test-token");
        let result = fetch_pr_by_head("owner/repo", "nonexistent", &creds).await;

        assert!(result.is_ok());
        assert!(result.unwrap().is_none());

        mock.assert_async().await;
    }

    #[tokio::test]
    #[serial]
    async fn test_fetch_pr_by_head_rate_limited() {
        let mut server = Server::new_async().await;

        let mock = server
            .mock("GET", "/repos/owner/repo/pulls")
            .match_query(mockito::Matcher::Any)
            .with_status(429)
            .with_body("rate limit exceeded")
            .create_async()
            .await;

        std::env::set_var("GITHUB_API_BASE", server.url());

        let creds = Credentials::new("test-token");
        let result = fetch_pr_by_head("owner/repo", "feature", &creds).await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("rate limit"));

        mock.assert_async().await;
    }

    #[tokio::test]
    #[serial]
    async fn test_fetch_prs_by_base_multiple() {
        let mut server = Server::new_async().await;

        let pr1 = make_pr_json(1, "feature-1", "main", "PR 1");
        let pr2 = make_pr_json(2, "feature-2", "main", "PR 2");

        let mock = server
            .mock("GET", "/repos/owner/repo/pulls")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("state".into(), "open".into()),
                mockito::Matcher::UrlEncoded("base".into(), "main".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(format!("[{}, {}]", pr1, pr2))
            .create_async()
            .await;

        std::env::set_var("GITHUB_API_BASE", server.url());

        let creds = Credentials::new("test-token");
        let result = fetch_prs_by_base("owner/repo", "main", &creds).await;

        assert!(result.is_ok());
        let prs = result.unwrap();
        assert_eq!(prs.len(), 2);

        mock.assert_async().await;
    }

    #[tokio::test]
    #[serial]
    async fn test_fetch_prs_by_base_empty() {
        let mut server = Server::new_async().await;

        let mock = server
            .mock("GET", "/repos/owner/repo/pulls")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("state".into(), "open".into()),
                mockito::Matcher::UrlEncoded("base".into(), "feature".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("[]")
            .create_async()
            .await;

        std::env::set_var("GITHUB_API_BASE", server.url());

        let creds = Credentials::new("test-token");
        let result = fetch_prs_by_base("owner/repo", "feature", &creds).await;

        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());

        mock.assert_async().await;
    }

    #[tokio::test]
    #[serial]
    async fn test_fetch_all_open_prs_single_page() {
        let mut server = Server::new_async().await;

        let pr1 = make_pr_json(1, "feature-1", "main", "PR 1");
        let pr2 = make_pr_json(2, "feature-2", "main", "PR 2");

        let mock = server
            .mock("GET", "/repos/owner/repo/pulls")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("state".into(), "open".into()),
                mockito::Matcher::UrlEncoded("per_page".into(), "100".into()),
                mockito::Matcher::UrlEncoded("page".into(), "1".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(format!("[{}, {}]", pr1, pr2))
            .create_async()
            .await;

        std::env::set_var("GITHUB_API_BASE", server.url());

        let creds = Credentials::new("test-token");
        let result = fetch_all_open_prs("owner/repo", &creds).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 2);

        mock.assert_async().await;
    }

    #[tokio::test]
    #[serial]
    async fn test_fetch_all_open_prs_pagination() {
        let mut server = Server::new_async().await;

        // Generate 100 PRs for page 1 (triggers pagination)
        let page1_prs: Vec<String> = (1..=100)
            .map(|i| make_pr_json(i, &format!("feature-{}", i), "main", &format!("PR {}", i)))
            .collect();
        let page1_body = format!("[{}]", page1_prs.join(","));

        // Page 2 has fewer than 100, indicating end
        let pr101 = make_pr_json(101, "feature-101", "main", "PR 101");

        let mock_page1 = server
            .mock("GET", "/repos/owner/repo/pulls")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("state".into(), "open".into()),
                mockito::Matcher::UrlEncoded("per_page".into(), "100".into()),
                mockito::Matcher::UrlEncoded("page".into(), "1".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(page1_body)
            .create_async()
            .await;

        let mock_page2 = server
            .mock("GET", "/repos/owner/repo/pulls")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("state".into(), "open".into()),
                mockito::Matcher::UrlEncoded("per_page".into(), "100".into()),
                mockito::Matcher::UrlEncoded("page".into(), "2".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(format!("[{}]", pr101))
            .create_async()
            .await;

        std::env::set_var("GITHUB_API_BASE", server.url());

        let creds = Credentials::new("test-token");
        let result = fetch_all_open_prs("owner/repo", &creds).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 101);

        mock_page1.assert_async().await;
        mock_page2.assert_async().await;
    }

    #[tokio::test]
    #[serial]
    async fn test_discover_stack_batch_fetch() {
        let mut server = Server::new_async().await;

        // Create a 3-PR stack
        let pr1 = make_pr_json(1, "feature-1", "main", "PR 1");
        let pr2 = make_pr_json(2, "feature-2", "feature-1", "PR 2");
        let pr3 = make_pr_json(3, "feature-3", "feature-2", "PR 3");

        // Single batch fetch should be enough
        let mock = server
            .mock("GET", "/repos/owner/repo/pulls")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("state".into(), "open".into()),
                mockito::Matcher::UrlEncoded("per_page".into(), "100".into()),
                mockito::Matcher::UrlEncoded("page".into(), "1".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(format!("[{}, {}, {}]", pr1, pr2, pr3))
            .expect(1) // Should only be called once!
            .create_async()
            .await;

        std::env::set_var("GITHUB_API_BASE", server.url());

        let creds = Credentials::new("test-token");

        // Create starting PR
        let starting_pr = PullRequest::new_for_test(
            2,
            "feature-2",
            "feature-1",
            "PR 2",
            PullRequestStatus::Open,
            false,
            None,
            vec![],
        );

        let result = discover_stack("owner/repo", starting_pr, "main", &creds).await;

        assert!(result.is_ok());
        let stack = result.unwrap();
        assert_eq!(stack.len(), 3);
        assert_eq!(stack[0].number(), 1); // bottom
        assert_eq!(stack[1].number(), 2);
        assert_eq!(stack[2].number(), 3); // top

        mock.assert_async().await;
    }
}
