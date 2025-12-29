//! Stack discovery via GitHub API
//!
//! Walks PR base/head chains to discover full stack structure without
//! requiring identifier patterns in PR titles.

use crate::api::{github_api_base, PullRequest};
use crate::Credentials;
use reqwest::Client;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::time::Duration;

/// Build a GET request with auth headers
fn build_request(client: &Client, creds: &Credentials, url: &str) -> reqwest::RequestBuilder {
    client
        .get(url)
        .timeout(Duration::from_secs(10))
        .header("Authorization", format!("token {}", creds.token))
        .header("User-Agent", "luqven/gh-stack")
        .header("Accept", "application/vnd.github.v3+json")
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

/// Fetch all open PRs in a repository.
///
/// # Arguments
/// * `repo` - Repository in "owner/repo" format
/// * `creds` - GitHub credentials
pub async fn fetch_all_open_prs(
    repo: &str,
    creds: &Credentials,
) -> Result<Vec<PullRequest>, Box<dyn Error>> {
    let client = Client::new();

    // Fetch up to 100 open PRs (pagination could be added for larger repos)
    let url = format!(
        "{}/repos/{}/pulls?state=open&per_page=100",
        github_api_base(),
        repo
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
    Ok(prs)
}

/// Discover the full stack by walking PR chain from a starting PR.
///
/// Walks UP via base branches (finding ancestors) and DOWN via child PRs
/// (finding descendants) until the full connected stack is discovered.
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

        // Try to find a PR with this branch as its head
        if let Some(pr) = fetch_pr_by_head(repo, &base, creds).await? {
            let pr_base = pr.base().to_string();
            visited.insert(pr.head().to_string(), pr);
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

        // Find all PRs that target this branch as their base
        let children = fetch_prs_by_base(repo, &head, creds).await?;
        for child in children {
            if !visited.contains_key(child.head()) {
                let child_head = child.head().to_string();
                visited.insert(child_head.clone(), child);
                down_queue.push(child_head);
            }
        }
    }

    // Sort PRs by their position in the stack (bottom to top)
    let prs: Vec<PullRequest> = visited.into_values().collect();
    Ok(sort_stack(prs, trunk))
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
/// each group as a separate stack.
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

    if all_prs.is_empty() {
        return Ok(vec![]);
    }

    // Build adjacency: base -> list of PRs targeting that base
    let mut base_to_prs: HashMap<String, Vec<&PullRequest>> = HashMap::new();
    for pr in &all_prs {
        base_to_prs
            .entry(pr.base().to_string())
            .or_default()
            .push(pr);
    }

    // Find root PRs (those whose base is trunk)
    let roots: Vec<&PullRequest> = all_prs.iter().filter(|pr| pr.base() == trunk).collect();

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

    Ok(stacks)
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

    #[test]
    fn test_sort_stack_linear() {
        let pr1 = PullRequest::new_for_test(
            1,
            "feature-1",
            "main",
            "PR 1",
            PullRequestStatus::Open,
            false,
            None,
            vec![],
        );
        let pr2 = PullRequest::new_for_test(
            2,
            "feature-2",
            "feature-1",
            "PR 2",
            PullRequestStatus::Open,
            false,
            None,
            vec![],
        );
        let pr3 = PullRequest::new_for_test(
            3,
            "feature-3",
            "feature-2",
            "PR 3",
            PullRequestStatus::Open,
            false,
            None,
            vec![],
        );

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
        let pr = PullRequest::new_for_test(
            1,
            "feature",
            "main",
            "PR 1",
            PullRequestStatus::Open,
            false,
            None,
            vec![],
        );

        let sorted = sort_stack(vec![pr], "main");
        assert_eq!(sorted.len(), 1);
        assert_eq!(sorted[0].number(), 1);
    }

    #[test]
    fn test_sort_stack_empty() {
        let sorted = sort_stack(vec![], "main");
        assert!(sorted.is_empty());
    }

    #[tokio::test]
    #[serial]
    async fn test_fetch_all_open_prs() {
        let mut server = Server::new_async().await;

        let pr1 = make_pr_json(1, "feature-1", "main", "PR 1");
        let pr2 = make_pr_json(2, "feature-2", "main", "PR 2");

        let mock = server
            .mock("GET", "/repos/owner/repo/pulls")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("state".into(), "open".into()),
                mockito::Matcher::UrlEncoded("per_page".into(), "100".into()),
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
}
