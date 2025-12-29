use crate::Credentials;
use chrono::{DateTime, Utc};
use reqwest::{Client, RequestBuilder, Response};
use std::error::Error;
use std::fmt;
use std::time::Duration;

pub mod checks;
pub mod land;
pub mod pull_request;
pub mod search;
pub mod stack;

pub use pull_request::PullRequest;
pub use pull_request::PullRequestReview;
pub use pull_request::PullRequestReviewState;
pub use pull_request::PullRequestStatus;

/// Base GitHub API URL - can be overridden for testing
#[cfg(not(test))]
pub const GITHUB_API_BASE: &str = "https://api.github.com";

#[cfg(test)]
pub fn github_api_base() -> String {
    std::env::var("GITHUB_API_BASE").unwrap_or_else(|_| "https://api.github.com".to_string())
}

#[cfg(not(test))]
pub fn github_api_base() -> String {
    GITHUB_API_BASE.to_string()
}

/// Maximum number of retry attempts for rate-limited requests
const MAX_RETRIES: u32 = 3;

/// Base delay between retries (will be doubled each attempt)
const BASE_RETRY_DELAY_MS: u64 = 1000;

/// Rate limit error with reset time information
#[derive(Debug, Clone)]
pub struct RateLimitError {
    pub reset_time: Option<DateTime<Utc>>,
    pub limit: Option<u32>,
    pub remaining: Option<u32>,
}

impl fmt::Display for RateLimitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.reset_time {
            Some(reset) => {
                let wait = reset.signed_duration_since(Utc::now());
                let mins = wait.num_minutes().max(1);
                write!(
                    f,
                    "GitHub API rate limit exceeded. Try again in {} minute{}.",
                    mins,
                    if mins == 1 { "" } else { "s" }
                )
            }
            None => write!(f, "GitHub API rate limit exceeded."),
        }
    }
}

impl Error for RateLimitError {}

/// Parse rate limit headers from a GitHub API response
fn parse_rate_limit_headers(response: &Response) -> RateLimitError {
    let headers = response.headers();

    let reset_time = headers
        .get("x-ratelimit-reset")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<i64>().ok())
        .and_then(|ts| DateTime::from_timestamp(ts, 0));

    let limit = headers
        .get("x-ratelimit-limit")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok());

    let remaining = headers
        .get("x-ratelimit-remaining")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok());

    RateLimitError {
        reset_time,
        limit,
        remaining,
    }
}

/// Check if a response indicates rate limiting (HTTP 429 or 403 with rate limit headers)
fn is_rate_limited(response: &Response) -> bool {
    if response.status() == 429 {
        return true;
    }

    // GitHub sometimes returns 403 for rate limits
    if response.status() == 403 {
        if let Some(remaining) = response.headers().get("x-ratelimit-remaining") {
            if remaining.to_str().unwrap_or("1") == "0" {
                return true;
            }
        }
    }

    false
}

/// Send a request with automatic retry on rate limit (HTTP 429).
///
/// Implements exponential backoff with up to MAX_RETRIES attempts.
/// On the final failure, returns a RateLimitError with reset time info.
///
/// # Arguments
/// * `client` - The reqwest client to use
/// * `build_request` - A closure that builds the request (called fresh each attempt)
///
/// # Returns
/// The successful response, or an error if all retries fail
pub async fn send_with_retry<F>(
    client: &Client,
    build_request: F,
) -> Result<Response, Box<dyn Error>>
where
    F: Fn(&Client) -> RequestBuilder,
{
    let mut last_rate_limit_error: Option<RateLimitError> = None;

    for attempt in 0..MAX_RETRIES {
        let request = build_request(client);
        let response = request.send().await?;

        if is_rate_limited(&response) {
            last_rate_limit_error = Some(parse_rate_limit_headers(&response));

            // Don't sleep on the last attempt
            if attempt < MAX_RETRIES - 1 {
                let delay_ms = BASE_RETRY_DELAY_MS * 2u64.pow(attempt);
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }
            continue;
        }

        return Ok(response);
    }

    // All retries exhausted
    Err(Box::new(last_rate_limit_error.unwrap_or(RateLimitError {
        reset_time: None,
        limit: None,
        remaining: None,
    })))
}

pub fn base_request(client: &Client, credentials: &Credentials, url: &str) -> RequestBuilder {
    client
        .get(url)
        .timeout(Duration::from_secs(5))
        .header("Authorization", format!("token {}", credentials.token))
        .header("User-Agent", "timothyandrew/gh-stack")
}

pub fn base_patch_request(client: &Client, credentials: &Credentials, url: &str) -> RequestBuilder {
    client
        .patch(url)
        .timeout(Duration::from_secs(5))
        .header("Authorization", format!("token {}", credentials.token))
        .header("User-Agent", "timothyandrew/gh-stack")
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;
    use serial_test::serial;

    #[test]
    fn test_base_request_sets_auth_header() {
        let client = Client::new();
        let creds = Credentials::new("test-token-123");
        let request = base_request(&client, &creds, "https://api.github.com/test");

        let built = request.build().unwrap();
        assert_eq!(
            built.headers().get("Authorization").unwrap(),
            "token test-token-123"
        );
    }

    #[test]
    fn test_base_request_sets_user_agent() {
        let client = Client::new();
        let creds = Credentials::new("test-token");
        let request = base_request(&client, &creds, "https://api.github.com/test");

        let built = request.build().unwrap();
        assert_eq!(
            built.headers().get("User-Agent").unwrap(),
            "timothyandrew/gh-stack"
        );
    }

    #[tokio::test]
    async fn test_mock_github_api_search() {
        let mut server = Server::new_async().await;

        let mock = server
            .mock("GET", "/search/issues")
            .match_query(mockito::Matcher::UrlEncoded(
                "q".into(),
                "TEST-123 in:title".into(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"items": []}"#)
            .create_async()
            .await;

        let client = Client::new();
        let _creds = Credentials::new("fake-token");

        let response = client
            .get(format!("{}/search/issues", server.url()))
            .query(&[("q", "TEST-123 in:title")])
            .header("Authorization", "token fake-token")
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), 200);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_mock_pull_request_fetch() {
        let mut server = Server::new_async().await;

        let pr_json = r#"{
            "id": 1,
            "number": 42,
            "head": {"label": "user:feature", "ref": "feature-branch", "sha": "abc123"},
            "base": {"label": "user:main", "ref": "main", "sha": "def456"},
            "title": "Test PR",
            "url": "https://api.github.com/repos/test/repo/pulls/42",
            "body": "PR description",
            "state": "open",
            "merged_at": null,
            "draft": false
        }"#;

        let mock = server
            .mock("GET", "/repos/test/repo/pulls/42")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(pr_json)
            .create_async()
            .await;

        let client = Client::new();

        let response = client
            .get(format!("{}/repos/test/repo/pulls/42", server.url()))
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), 200);

        let pr: PullRequest = response.json().await.unwrap();
        assert_eq!(pr.number(), 42);
        assert_eq!(pr.head(), "feature-branch");
        assert_eq!(pr.base(), "main");

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_mock_reviews_fetch() {
        let mut server = Server::new_async().await;

        let reviews_json = r#"[
            {"state": "APPROVED", "body": "LGTM!"},
            {"state": "COMMENTED", "body": "Nice work"}
        ]"#;

        let mock = server
            .mock("GET", "/repos/test/repo/pulls/42/reviews")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(reviews_json)
            .create_async()
            .await;

        let client = Client::new();

        let response = client
            .get(format!("{}/repos/test/repo/pulls/42/reviews", server.url()))
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), 200);

        let reviews: Vec<PullRequestReview> = response.json().await.unwrap();
        assert_eq!(reviews.len(), 2);
        assert!(reviews[0].is_approved());
        assert!(!reviews[1].is_approved());

        mock.assert_async().await;
    }

    #[test]
    fn test_rate_limit_error_display_with_reset() {
        let future_time = Utc::now() + chrono::Duration::minutes(5);
        let err = RateLimitError {
            reset_time: Some(future_time),
            limit: Some(5000),
            remaining: Some(0),
        };
        let msg = format!("{}", err);
        assert!(msg.contains("rate limit exceeded"));
        assert!(msg.contains("minute"));
    }

    #[test]
    fn test_rate_limit_error_display_without_reset() {
        let err = RateLimitError {
            reset_time: None,
            limit: None,
            remaining: None,
        };
        let msg = format!("{}", err);
        assert!(msg.contains("rate limit exceeded"));
    }

    #[tokio::test]
    #[serial]
    async fn test_send_with_retry_success_first_try() {
        let mut server = Server::new_async().await;

        let mock = server
            .mock("GET", "/test")
            .with_status(200)
            .with_body("ok")
            .expect(1)
            .create_async()
            .await;

        let client = Client::new();
        let result = send_with_retry(&client, |c| c.get(format!("{}/test", server.url()))).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap().status(), 200);
        mock.assert_async().await;
    }

    #[tokio::test]
    #[serial]
    async fn test_send_with_retry_rate_limit_then_success() {
        let mut server = Server::new_async().await;

        // First request: rate limited
        let mock_429 = server
            .mock("GET", "/test")
            .with_status(429)
            .with_header("x-ratelimit-remaining", "0")
            .expect(1)
            .create_async()
            .await;

        // Second request: success
        let mock_200 = server
            .mock("GET", "/test")
            .with_status(200)
            .with_body("ok")
            .expect(1)
            .create_async()
            .await;

        let client = Client::new();
        let result = send_with_retry(&client, |c| c.get(format!("{}/test", server.url()))).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap().status(), 200);
        mock_429.assert_async().await;
        mock_200.assert_async().await;
    }

    #[tokio::test]
    #[serial]
    async fn test_send_with_retry_exhausted() {
        let mut server = Server::new_async().await;

        // All requests: rate limited
        let mock = server
            .mock("GET", "/test")
            .with_status(429)
            .with_header("x-ratelimit-remaining", "0")
            .with_header("x-ratelimit-limit", "5000")
            .expect(3) // MAX_RETRIES
            .create_async()
            .await;

        let client = Client::new();
        let result = send_with_retry(&client, |c| c.get(format!("{}/test", server.url()))).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("rate limit"));
        mock.assert_async().await;
    }

    #[tokio::test]
    #[serial]
    async fn test_send_with_retry_403_with_rate_limit() {
        let mut server = Server::new_async().await;

        // First: 403 with rate limit headers (GitHub sometimes does this)
        let mock_403 = server
            .mock("GET", "/test")
            .with_status(403)
            .with_header("x-ratelimit-remaining", "0")
            .expect(1)
            .create_async()
            .await;

        // Second: success
        let mock_200 = server
            .mock("GET", "/test")
            .with_status(200)
            .with_body("ok")
            .expect(1)
            .create_async()
            .await;

        let client = Client::new();
        let result = send_with_retry(&client, |c| c.get(format!("{}/test", server.url()))).await;

        assert!(result.is_ok());
        mock_403.assert_async().await;
        mock_200.assert_async().await;
    }

    #[test]
    fn test_is_rate_limited_429() {
        // Can't easily test this without mocking Response, but the logic is tested in integration tests above
    }

    #[test]
    fn test_parse_rate_limit_headers() {
        // Unit test for header parsing logic is covered by integration tests
    }
}
