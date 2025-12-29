//! GitHub Checks API for CI status
//!
//! This module provides functions to fetch CI check status and PR mergeable state
//! from the GitHub API.

use reqwest::Client;
use serde::Deserialize;
use std::error::Error;
use std::time::Duration;

use crate::Credentials;

/// Overall state of CI checks for a commit
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CheckState {
    /// All checks passed
    Success,
    /// At least one check failed
    Failure,
    /// Checks are still running
    Pending,
    /// No checks or all skipped/neutral
    Neutral,
}

/// Aggregated check status for a commit
#[derive(Debug, Clone)]
pub struct CheckStatus {
    pub state: CheckState,
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub pending: usize,
}

impl CheckStatus {
    /// Create a neutral status (no checks)
    pub fn neutral() -> Self {
        CheckStatus {
            state: CheckState::Neutral,
            total: 0,
            passed: 0,
            failed: 0,
            pending: 0,
        }
    }
}

/// Response from GitHub check-runs API
#[derive(Deserialize, Debug)]
struct CheckRunsResponse {
    total_count: usize,
    check_runs: Vec<CheckRun>,
}

/// Individual check run from GitHub API
#[derive(Deserialize, Debug)]
struct CheckRun {
    /// "completed", "in_progress", "queued", "pending"
    status: String,
    /// "success", "failure", "neutral", "cancelled", "skipped", "timed_out", "action_required"
    conclusion: Option<String>,
}

/// Response from GitHub PR API (for mergeable field)
#[derive(Deserialize, Debug)]
struct PrMergeableResponse {
    mergeable: Option<bool>,
}

fn build_get_request(
    client: &Client,
    credentials: &Credentials,
    url: &str,
) -> reqwest::RequestBuilder {
    client
        .get(url)
        .timeout(Duration::from_secs(10))
        .header("Authorization", format!("token {}", credentials.token))
        .header("User-Agent", "luqven/gh-stack")
        .header("Accept", "application/vnd.github.v3+json")
}

/// Fetch check status for a commit SHA
///
/// # Arguments
/// * `sha` - The commit SHA to check
/// * `repo` - Repository in "owner/repo" format
/// * `credentials` - GitHub credentials
pub async fn fetch_check_status(
    sha: &str,
    repo: &str,
    credentials: &Credentials,
) -> Result<CheckStatus, Box<dyn Error>> {
    let client = Client::new();
    let url = format!(
        "{}/repos/{}/commits/{}/check-runs",
        super::github_api_base(),
        repo,
        sha
    );

    let response = build_get_request(&client, credentials, &url).send().await?;

    if response.status() == 429 {
        return Err("GitHub API rate limit exceeded".into());
    }

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(format!("Failed to fetch check status ({}): {}", status, text).into());
    }

    let check_runs: CheckRunsResponse = response.json().await?;
    Ok(parse_check_runs(&check_runs))
}

/// Parse check runs response into aggregated status
fn parse_check_runs(response: &CheckRunsResponse) -> CheckStatus {
    if response.total_count == 0 {
        return CheckStatus::neutral();
    }

    let mut passed = 0;
    let mut failed = 0;
    let mut pending = 0;

    for run in &response.check_runs {
        match run.status.as_str() {
            "completed" => {
                match run.conclusion.as_deref() {
                    Some("success") | Some("neutral") | Some("skipped") => passed += 1,
                    Some("failure")
                    | Some("timed_out")
                    | Some("cancelled")
                    | Some("action_required") => failed += 1,
                    _ => pending += 1, // Unknown conclusion treated as pending
                }
            }
            _ => pending += 1, // in_progress, queued, pending, or unknown
        }
    }

    let state = if failed > 0 {
        CheckState::Failure
    } else if pending > 0 {
        CheckState::Pending
    } else if passed > 0 {
        CheckState::Success
    } else {
        CheckState::Neutral
    };

    CheckStatus {
        state,
        total: response.total_count,
        passed,
        failed,
        pending,
    }
}

/// Fetch mergeable status for a PR
///
/// GitHub computes mergeability asynchronously, so this may return None
/// if GitHub is still calculating.
///
/// # Arguments
/// * `pr_number` - The PR number
/// * `repo` - Repository in "owner/repo" format
/// * `credentials` - GitHub credentials
pub async fn fetch_mergeable_status(
    pr_number: usize,
    repo: &str,
    credentials: &Credentials,
) -> Result<Option<bool>, Box<dyn Error>> {
    let client = Client::new();
    let url = format!(
        "{}/repos/{}/pulls/{}",
        super::github_api_base(),
        repo,
        pr_number
    );

    let response = build_get_request(&client, credentials, &url).send().await?;

    if response.status() == 429 {
        return Err("GitHub API rate limit exceeded".into());
    }

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(format!("Failed to fetch PR mergeable status ({}): {}", status, text).into());
    }

    let pr: PrMergeableResponse = response.json().await?;
    Ok(pr.mergeable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;
    use serial_test::serial;

    // === Unit tests for parsing ===

    #[test]
    fn test_parse_check_runs_all_success() {
        let response = CheckRunsResponse {
            total_count: 3,
            check_runs: vec![
                CheckRun {
                    status: "completed".to_string(),
                    conclusion: Some("success".to_string()),
                },
                CheckRun {
                    status: "completed".to_string(),
                    conclusion: Some("success".to_string()),
                },
                CheckRun {
                    status: "completed".to_string(),
                    conclusion: Some("success".to_string()),
                },
            ],
        };

        let status = parse_check_runs(&response);
        assert_eq!(status.state, CheckState::Success);
        assert_eq!(status.total, 3);
        assert_eq!(status.passed, 3);
        assert_eq!(status.failed, 0);
        assert_eq!(status.pending, 0);
    }

    #[test]
    fn test_parse_check_runs_mixed() {
        let response = CheckRunsResponse {
            total_count: 3,
            check_runs: vec![
                CheckRun {
                    status: "completed".to_string(),
                    conclusion: Some("success".to_string()),
                },
                CheckRun {
                    status: "completed".to_string(),
                    conclusion: Some("failure".to_string()),
                },
                CheckRun {
                    status: "in_progress".to_string(),
                    conclusion: None,
                },
            ],
        };

        let status = parse_check_runs(&response);
        assert_eq!(status.state, CheckState::Failure); // Failure takes precedence
        assert_eq!(status.total, 3);
        assert_eq!(status.passed, 1);
        assert_eq!(status.failed, 1);
        assert_eq!(status.pending, 1);
    }

    #[test]
    fn test_parse_check_runs_all_pending() {
        let response = CheckRunsResponse {
            total_count: 2,
            check_runs: vec![
                CheckRun {
                    status: "in_progress".to_string(),
                    conclusion: None,
                },
                CheckRun {
                    status: "queued".to_string(),
                    conclusion: None,
                },
            ],
        };

        let status = parse_check_runs(&response);
        assert_eq!(status.state, CheckState::Pending);
        assert_eq!(status.total, 2);
        assert_eq!(status.passed, 0);
        assert_eq!(status.failed, 0);
        assert_eq!(status.pending, 2);
    }

    #[test]
    fn test_parse_check_runs_empty() {
        let response = CheckRunsResponse {
            total_count: 0,
            check_runs: vec![],
        };

        let status = parse_check_runs(&response);
        assert_eq!(status.state, CheckState::Neutral);
        assert_eq!(status.total, 0);
    }

    #[test]
    fn test_parse_check_runs_neutral_conclusion() {
        let response = CheckRunsResponse {
            total_count: 2,
            check_runs: vec![
                CheckRun {
                    status: "completed".to_string(),
                    conclusion: Some("neutral".to_string()),
                },
                CheckRun {
                    status: "completed".to_string(),
                    conclusion: Some("skipped".to_string()),
                },
            ],
        };

        let status = parse_check_runs(&response);
        assert_eq!(status.state, CheckState::Success); // Neutral/skipped count as passed
        assert_eq!(status.passed, 2);
    }

    #[test]
    fn test_parse_check_runs_timed_out() {
        let response = CheckRunsResponse {
            total_count: 1,
            check_runs: vec![CheckRun {
                status: "completed".to_string(),
                conclusion: Some("timed_out".to_string()),
            }],
        };

        let status = parse_check_runs(&response);
        assert_eq!(status.state, CheckState::Failure);
        assert_eq!(status.failed, 1);
    }

    #[test]
    fn test_parse_check_runs_cancelled() {
        let response = CheckRunsResponse {
            total_count: 1,
            check_runs: vec![CheckRun {
                status: "completed".to_string(),
                conclusion: Some("cancelled".to_string()),
            }],
        };

        let status = parse_check_runs(&response);
        assert_eq!(status.state, CheckState::Failure);
        assert_eq!(status.failed, 1);
    }

    // === Async/mock tests ===

    #[tokio::test]
    #[serial]
    async fn test_fetch_check_status_success() {
        let mut server = Server::new_async().await;

        let mock = server
            .mock("GET", "/repos/owner/repo/commits/abc123/check-runs")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "total_count": 2,
                    "check_runs": [
                        {"status": "completed", "conclusion": "success"},
                        {"status": "completed", "conclusion": "success"}
                    ]
                }"#,
            )
            .create_async()
            .await;

        std::env::set_var("GITHUB_API_BASE", server.url());

        let creds = Credentials::new("test-token");
        let result = fetch_check_status("abc123", "owner/repo", &creds).await;

        assert!(result.is_ok());
        let status = result.unwrap();
        assert_eq!(status.state, CheckState::Success);
        assert_eq!(status.passed, 2);

        mock.assert_async().await;
    }

    #[tokio::test]
    #[serial]
    async fn test_fetch_check_status_rate_limited() {
        let mut server = Server::new_async().await;

        let mock = server
            .mock("GET", "/repos/owner/repo/commits/abc123/check-runs")
            .with_status(429)
            .with_body("rate limit exceeded")
            .create_async()
            .await;

        std::env::set_var("GITHUB_API_BASE", server.url());

        let creds = Credentials::new("test-token");
        let result = fetch_check_status("abc123", "owner/repo", &creds).await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("rate limit"));

        mock.assert_async().await;
    }

    #[tokio::test]
    #[serial]
    async fn test_fetch_check_status_api_error() {
        let mut server = Server::new_async().await;

        let mock = server
            .mock("GET", "/repos/owner/repo/commits/abc123/check-runs")
            .with_status(404)
            .with_body("not found")
            .create_async()
            .await;

        std::env::set_var("GITHUB_API_BASE", server.url());

        let creds = Credentials::new("test-token");
        let result = fetch_check_status("abc123", "owner/repo", &creds).await;

        assert!(result.is_err());

        mock.assert_async().await;
    }

    #[tokio::test]
    #[serial]
    async fn test_fetch_mergeable_status_true() {
        let mut server = Server::new_async().await;

        let mock = server
            .mock("GET", "/repos/owner/repo/pulls/123")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"mergeable": true}"#)
            .create_async()
            .await;

        std::env::set_var("GITHUB_API_BASE", server.url());

        let creds = Credentials::new("test-token");
        let result = fetch_mergeable_status(123, "owner/repo", &creds).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some(true));

        mock.assert_async().await;
    }

    #[tokio::test]
    #[serial]
    async fn test_fetch_mergeable_status_false() {
        let mut server = Server::new_async().await;

        let mock = server
            .mock("GET", "/repos/owner/repo/pulls/123")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"mergeable": false}"#)
            .create_async()
            .await;

        std::env::set_var("GITHUB_API_BASE", server.url());

        let creds = Credentials::new("test-token");
        let result = fetch_mergeable_status(123, "owner/repo", &creds).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some(false));

        mock.assert_async().await;
    }

    #[tokio::test]
    #[serial]
    async fn test_fetch_mergeable_status_null() {
        let mut server = Server::new_async().await;

        let mock = server
            .mock("GET", "/repos/owner/repo/pulls/123")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"mergeable": null}"#)
            .create_async()
            .await;

        std::env::set_var("GITHUB_API_BASE", server.url());

        let creds = Credentials::new("test-token");
        let result = fetch_mergeable_status(123, "owner/repo", &creds).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), None);

        mock.assert_async().await;
    }

    #[tokio::test]
    #[serial]
    async fn test_fetch_mergeable_status_rate_limited() {
        let mut server = Server::new_async().await;

        let mock = server
            .mock("GET", "/repos/owner/repo/pulls/123")
            .with_status(429)
            .with_body("rate limit exceeded")
            .create_async()
            .await;

        std::env::set_var("GITHUB_API_BASE", server.url());

        let creds = Credentials::new("test-token");
        let result = fetch_mergeable_status(123, "owner/repo", &creds).await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("rate limit"));

        mock.assert_async().await;
    }
}
