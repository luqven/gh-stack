//! GitHub API methods for creating PRs
//!
//! This module provides functionality to create pull requests via the GitHub API,
//! eliminating the need for the `gh` CLI dependency.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::time::Duration;

use crate::Credentials;

/// Request body for creating a PR
#[derive(Serialize, Debug)]
struct CreatePrRequest<'a> {
    title: &'a str,
    head: &'a str,
    base: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    body: Option<&'a str>,
}

/// Response from PR creation endpoint
#[derive(Deserialize, Debug)]
struct CreatePrResponse {
    number: usize,
    html_url: String,
}

/// Create a new pull request via GitHub API
///
/// # Arguments
/// * `repository` - Repository in "owner/repo" format
/// * `head` - Head branch name (the branch with changes)
/// * `base` - Base branch name (the branch to merge into)
/// * `title` - PR title
/// * `body` - Optional PR body/description
/// * `credentials` - GitHub credentials
///
/// # Returns
/// Tuple of (pr_number, html_url) on success
///
/// # Errors
/// Returns an error if the API request fails or returns a non-success status
pub async fn create_pr(
    repository: &str,
    head: &str,
    base: &str,
    title: &str,
    body: Option<&str>,
    credentials: &Credentials,
) -> Result<(usize, String), Box<dyn Error>> {
    let client = Client::new();
    let url = format!("{}/repos/{}/pulls", super::github_api_base(), repository);

    let request_body = CreatePrRequest {
        title,
        head,
        base,
        body,
    };

    let response = client
        .post(&url)
        .timeout(Duration::from_secs(30))
        .header("Authorization", format!("token {}", credentials.token))
        .header("User-Agent", "luqven/gh-stack")
        .header("Accept", "application/vnd.github.v3+json")
        .json(&request_body)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(format!("Failed to create PR ({}): {}", status, text).into());
    }

    let pr: CreatePrResponse = response.json().await?;
    Ok((pr.number, pr.html_url))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;
    use serial_test::serial;

    #[tokio::test]
    #[serial]
    async fn test_create_pr_success() {
        let mut server = Server::new_async().await;

        let mock = server
            .mock("POST", "/repos/owner/repo/pulls")
            .match_body(mockito::Matcher::Json(serde_json::json!({
                "title": "Test PR",
                "head": "feature",
                "base": "main",
                "body": "PR body"
            })))
            .with_status(201)
            .with_body(r#"{"number": 123, "html_url": "https://github.com/owner/repo/pull/123"}"#)
            .create_async()
            .await;

        std::env::set_var("GITHUB_API_BASE", server.url());

        let creds = Credentials::new("test-token");
        let result = create_pr(
            "owner/repo",
            "feature",
            "main",
            "Test PR",
            Some("PR body"),
            &creds,
        )
        .await;

        assert!(result.is_ok());
        let (number, url) = result.unwrap();
        assert_eq!(number, 123);
        assert_eq!(url, "https://github.com/owner/repo/pull/123");
        mock.assert_async().await;
    }

    #[tokio::test]
    #[serial]
    async fn test_create_pr_without_body() {
        let mut server = Server::new_async().await;

        let mock = server
            .mock("POST", "/repos/owner/repo/pulls")
            .match_body(mockito::Matcher::Json(serde_json::json!({
                "title": "Test PR",
                "head": "feature",
                "base": "main"
            })))
            .with_status(201)
            .with_body(r#"{"number": 456, "html_url": "https://github.com/owner/repo/pull/456"}"#)
            .create_async()
            .await;

        std::env::set_var("GITHUB_API_BASE", server.url());

        let creds = Credentials::new("test-token");
        let result = create_pr("owner/repo", "feature", "main", "Test PR", None, &creds).await;

        assert!(result.is_ok());
        let (number, _) = result.unwrap();
        assert_eq!(number, 456);
        mock.assert_async().await;
    }

    #[tokio::test]
    #[serial]
    async fn test_create_pr_validation_error() {
        let mut server = Server::new_async().await;

        let mock = server
            .mock("POST", "/repos/owner/repo/pulls")
            .with_status(422)
            .with_body(r#"{"message": "Validation Failed", "errors": [{"resource": "PullRequest", "code": "custom", "message": "A pull request already exists"}]}"#)
            .create_async()
            .await;

        std::env::set_var("GITHUB_API_BASE", server.url());

        let creds = Credentials::new("test-token");
        let result = create_pr("owner/repo", "feature", "main", "Test PR", None, &creds).await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("422"));
        assert!(err.contains("Validation Failed"));
        mock.assert_async().await;
    }

    #[tokio::test]
    #[serial]
    async fn test_create_pr_unauthorized() {
        let mut server = Server::new_async().await;

        let mock = server
            .mock("POST", "/repos/owner/repo/pulls")
            .with_status(401)
            .with_body(r#"{"message": "Bad credentials"}"#)
            .create_async()
            .await;

        std::env::set_var("GITHUB_API_BASE", server.url());

        let creds = Credentials::new("bad-token");
        let result = create_pr("owner/repo", "feature", "main", "Test PR", None, &creds).await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("401"));
        mock.assert_async().await;
    }

    #[tokio::test]
    #[serial]
    async fn test_create_pr_with_identifier_in_body() {
        let mut server = Server::new_async().await;

        let mock = server
            .mock("POST", "/repos/owner/repo/pulls")
            .match_body(mockito::Matcher::Json(serde_json::json!({
                "title": "[STACK-123] My feature",
                "head": "feature",
                "base": "main",
                "body": "<!-- gh-stack:[STACK-123] -->"
            })))
            .with_status(201)
            .with_body(r#"{"number": 789, "html_url": "https://github.com/owner/repo/pull/789"}"#)
            .create_async()
            .await;

        std::env::set_var("GITHUB_API_BASE", server.url());

        let creds = Credentials::new("test-token");
        let result = create_pr(
            "owner/repo",
            "feature",
            "main",
            "[STACK-123] My feature",
            Some("<!-- gh-stack:[STACK-123] -->"),
            &creds,
        )
        .await;

        assert!(result.is_ok());
        let (number, _) = result.unwrap();
        assert_eq!(number, 789);
        mock.assert_async().await;
    }
}
