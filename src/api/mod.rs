use crate::Credentials;
use reqwest::{Client, RequestBuilder};
use std::time::Duration;

pub mod pull_request;
pub mod search;

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
        let creds = Credentials::new("fake-token");

        let response = client
            .get(format!("{}/search/issues", server.url()))
            .query(&[("q", "TEST-123 in:title")])
            .header("Authorization", format!("token {}", "fake-token"))
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
}
