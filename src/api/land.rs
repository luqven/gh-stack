//! GitHub API methods for landing PRs
//!
//! This module provides functions to:
//! - Update a PR's base branch
//! - Merge a PR using squash strategy
//! - Close a PR with a comment

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::time::Duration;

use crate::Credentials;

/// Request body for updating a PR's base branch
#[derive(Serialize, Debug)]
struct UpdatePrBaseRequest<'a> {
    base: &'a str,
}

/// Request body for merging a PR
#[derive(Serialize, Debug)]
struct MergePrRequest<'a> {
    merge_method: &'a str,
}

/// Request body for closing a PR
#[derive(Serialize, Debug)]
struct ClosePrRequest {
    state: &'static str,
}

/// Request body for adding a comment
#[derive(Serialize, Debug)]
struct AddCommentRequest<'a> {
    body: &'a str,
}

/// Response from merge endpoint
#[derive(Deserialize, Debug)]
#[allow(dead_code)]
struct MergeResponse {
    sha: String,
    merged: bool,
    message: String,
}

/// Response with HTML URL
#[derive(Deserialize, Debug)]
struct PrResponse {
    html_url: String,
}

fn build_request(client: &Client, credentials: &Credentials, url: &str) -> reqwest::RequestBuilder {
    client
        .patch(url)
        .timeout(Duration::from_secs(30))
        .header("Authorization", format!("token {}", credentials.token))
        .header("User-Agent", "luqven/gh-stack")
        .header("Accept", "application/vnd.github.v3+json")
}

fn build_put_request(
    client: &Client,
    credentials: &Credentials,
    url: &str,
) -> reqwest::RequestBuilder {
    client
        .put(url)
        .timeout(Duration::from_secs(30))
        .header("Authorization", format!("token {}", credentials.token))
        .header("User-Agent", "luqven/gh-stack")
        .header("Accept", "application/vnd.github.v3+json")
}

fn build_post_request(
    client: &Client,
    credentials: &Credentials,
    url: &str,
) -> reqwest::RequestBuilder {
    client
        .post(url)
        .timeout(Duration::from_secs(30))
        .header("Authorization", format!("token {}", credentials.token))
        .header("User-Agent", "luqven/gh-stack")
        .header("Accept", "application/vnd.github.v3+json")
}

/// Update a PR's base branch
///
/// # Arguments
/// * `pr_number` - The PR number
/// * `new_base` - The new base branch name (e.g., "main")
/// * `repository` - Repository in "owner/repo" format
/// * `credentials` - GitHub credentials
pub async fn update_pr_base(
    pr_number: usize,
    new_base: &str,
    repository: &str,
    credentials: &Credentials,
) -> Result<(), Box<dyn Error>> {
    let client = Client::new();
    let url = format!(
        "{}/repos/{}/pulls/{}",
        super::github_api_base(),
        repository,
        pr_number
    );

    let body = UpdatePrBaseRequest { base: new_base };
    let response = build_request(&client, credentials, &url)
        .json(&body)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(format!("Failed to update PR base ({}): {}", status, text).into());
    }

    Ok(())
}

/// Merge a PR using squash strategy
///
/// # Arguments
/// * `pr_number` - The PR number
/// * `repository` - Repository in "owner/repo" format
/// * `credentials` - GitHub credentials
///
/// # Returns
/// The HTML URL of the merged PR
pub async fn merge_pr(
    pr_number: usize,
    repository: &str,
    credentials: &Credentials,
) -> Result<String, Box<dyn Error>> {
    let client = Client::new();
    let url = format!(
        "{}/repos/{}/pulls/{}/merge",
        super::github_api_base(),
        repository,
        pr_number
    );

    let body = MergePrRequest {
        merge_method: "squash",
    };
    let response = build_put_request(&client, credentials, &url)
        .json(&body)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(format!("Failed to merge PR ({}): {}", status, text).into());
    }

    let merge_response: MergeResponse = response.json().await?;
    if !merge_response.merged {
        return Err(format!("PR was not merged: {}", merge_response.message).into());
    }

    // Get the PR HTML URL
    let pr_url = format!(
        "{}/repos/{}/pulls/{}",
        super::github_api_base(),
        repository,
        pr_number
    );
    let pr_response = client
        .get(&pr_url)
        .timeout(Duration::from_secs(10))
        .header("Authorization", format!("token {}", credentials.token))
        .header("User-Agent", "luqven/gh-stack")
        .send()
        .await?;

    let pr_data: PrResponse = pr_response.json().await?;
    Ok(pr_data.html_url)
}

/// Close a PR with a comment
///
/// # Arguments
/// * `pr_number` - The PR number
/// * `comment` - Comment to add before closing
/// * `repository` - Repository in "owner/repo" format
/// * `credentials` - GitHub credentials
pub async fn close_pr_with_comment(
    pr_number: usize,
    comment: &str,
    repository: &str,
    credentials: &Credentials,
) -> Result<(), Box<dyn Error>> {
    let client = Client::new();

    // First, add a comment
    let comment_url = format!(
        "{}/repos/{}/issues/{}/comments",
        super::github_api_base(),
        repository,
        pr_number
    );
    let comment_body = AddCommentRequest { body: comment };
    let comment_response = build_post_request(&client, credentials, &comment_url)
        .json(&comment_body)
        .send()
        .await?;

    if !comment_response.status().is_success() {
        let status = comment_response.status();
        let text = comment_response.text().await.unwrap_or_default();
        return Err(format!("Failed to add comment ({}): {}", status, text).into());
    }

    // Then close the PR
    let close_url = format!(
        "{}/repos/{}/pulls/{}",
        super::github_api_base(),
        repository,
        pr_number
    );
    let close_body = ClosePrRequest { state: "closed" };
    let close_response = build_request(&client, credentials, &close_url)
        .json(&close_body)
        .send()
        .await?;

    if !close_response.status().is_success() {
        let status = close_response.status();
        let text = close_response.text().await.unwrap_or_default();
        return Err(format!("Failed to close PR ({}): {}", status, text).into());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;
    use serial_test::serial;

    #[tokio::test]
    #[serial]
    async fn test_update_pr_base() {
        let mut server = Server::new_async().await;

        let mock = server
            .mock("PATCH", "/repos/owner/repo/pulls/123")
            .match_body(mockito::Matcher::Json(serde_json::json!({"base": "main"})))
            .with_status(200)
            .with_body("{}")
            .create_async()
            .await;

        std::env::set_var("GITHUB_API_BASE", server.url());

        let creds = Credentials::new("test-token");
        let result = update_pr_base(123, "main", "owner/repo", &creds).await;

        assert!(result.is_ok());
        mock.assert_async().await;
    }

    #[tokio::test]
    #[serial]
    async fn test_merge_pr() {
        let mut server = Server::new_async().await;

        let merge_mock = server
            .mock("PUT", "/repos/owner/repo/pulls/123/merge")
            .match_body(mockito::Matcher::Json(
                serde_json::json!({"merge_method": "squash"}),
            ))
            .with_status(200)
            .with_body(r#"{"sha": "abc123", "merged": true, "message": "Pull Request successfully merged"}"#)
            .create_async()
            .await;

        let pr_mock = server
            .mock("GET", "/repos/owner/repo/pulls/123")
            .with_status(200)
            .with_body(r#"{"html_url": "https://github.com/owner/repo/pull/123"}"#)
            .create_async()
            .await;

        std::env::set_var("GITHUB_API_BASE", server.url());

        let creds = Credentials::new("test-token");
        let result = merge_pr(123, "owner/repo", &creds).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "https://github.com/owner/repo/pull/123");
        merge_mock.assert_async().await;
        pr_mock.assert_async().await;
    }

    #[tokio::test]
    #[serial]
    async fn test_close_pr_with_comment() {
        let mut server = Server::new_async().await;

        let comment_mock = server
            .mock("POST", "/repos/owner/repo/issues/123/comments")
            .match_body(mockito::Matcher::Json(
                serde_json::json!({"body": "Landed via #456"}),
            ))
            .with_status(201)
            .with_body("{}")
            .create_async()
            .await;

        let close_mock = server
            .mock("PATCH", "/repos/owner/repo/pulls/123")
            .match_body(mockito::Matcher::Json(
                serde_json::json!({"state": "closed"}),
            ))
            .with_status(200)
            .with_body("{}")
            .create_async()
            .await;

        std::env::set_var("GITHUB_API_BASE", server.url());

        let creds = Credentials::new("test-token");
        let result = close_pr_with_comment(123, "Landed via #456", "owner/repo", &creds).await;

        assert!(result.is_ok());
        comment_mock.assert_async().await;
        close_mock.assert_async().await;
    }
}
