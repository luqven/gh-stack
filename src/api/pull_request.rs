use serde::Deserialize;
use serde::Serialize;
use std::error::Error;
use std::rc::Rc;

use crate::api::search;
use crate::{api, Credentials};

#[derive(Deserialize, Debug, Clone, PartialEq)]
#[allow(non_camel_case_types)]
pub enum PullRequestReviewState {
    APPROVED,
    PENDING,
    CHANGES_REQUESTED,
    DISMISSED,
    COMMENTED,
    MERGED,
}

#[derive(Deserialize, Debug, Clone)]
#[allow(dead_code)]
pub struct PullRequestReview {
    state: PullRequestReviewState,
    body: String,
}

impl PullRequestReview {
    /// Create a new PullRequestReview for testing purposes
    #[cfg(test)]
    pub fn new_for_test(state: PullRequestReviewState) -> Self {
        PullRequestReview {
            state,
            body: String::new(),
        }
    }

    pub fn is_approved(&self) -> bool {
        self.state == PullRequestReviewState::APPROVED
    }
}

#[derive(Deserialize, Debug, Clone)]
#[allow(dead_code)]
pub struct PullRequestRef {
    label: String,
    #[serde(rename = "ref")]
    gitref: String,
    sha: String,
}

#[derive(Deserialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum PullRequestStatus {
    #[serde(rename = "open")]
    Open,
    #[serde(rename = "closed")]
    Closed,
}

#[derive(Deserialize, Debug, Clone)]
#[allow(dead_code)]
pub struct PullRequest {
    id: usize,
    number: usize,
    head: PullRequestRef,
    base: PullRequestRef,
    title: String,
    url: String,
    body: Option<String>,
    state: PullRequestStatus,
    merged_at: Option<String>,
    updated_at: Option<String>,
    draft: bool,
    #[serde(skip)]
    reviews: Vec<PullRequestReview>,
}

impl PullRequest {
    /// Create a new PullRequest for testing purposes
    #[cfg(test)]
    pub fn new_for_test(
        number: usize,
        head: &str,
        base: &str,
        title: &str,
        state: PullRequestStatus,
        draft: bool,
        merged_at: Option<String>,
        reviews: Vec<PullRequestReview>,
    ) -> Self {
        PullRequest {
            id: number,
            number,
            head: PullRequestRef {
                label: format!("user:{}", head),
                gitref: head.to_string(),
                sha: "abc123".to_string(),
            },
            base: PullRequestRef {
                label: format!("user:{}", base),
                gitref: base.to_string(),
                sha: "def456".to_string(),
            },
            title: title.to_string(),
            url: format!("https://api.github.com/repos/test/repo/pulls/{}", number),
            body: None,
            state,
            merged_at,
            updated_at: None,
            draft,
            reviews,
        }
    }

    /// Create a new PullRequest for testing purposes with updated_at field
    #[cfg(test)]
    pub fn new_for_test_with_updated_at(
        number: usize,
        head: &str,
        base: &str,
        title: &str,
        state: PullRequestStatus,
        draft: bool,
        merged_at: Option<String>,
        updated_at: Option<String>,
        reviews: Vec<PullRequestReview>,
    ) -> Self {
        PullRequest {
            id: number,
            number,
            head: PullRequestRef {
                label: format!("user:{}", head),
                gitref: head.to_string(),
                sha: "abc123".to_string(),
            },
            base: PullRequestRef {
                label: format!("user:{}", base),
                gitref: base.to_string(),
                sha: "def456".to_string(),
            },
            title: title.to_string(),
            url: format!("https://api.github.com/repos/test/repo/pulls/{}", number),
            body: None,
            state,
            merged_at,
            updated_at,
            draft,
            reviews,
        }
    }

    pub fn head(&self) -> &str {
        &self.head.gitref
    }

    pub fn base(&self) -> &str {
        &self.base.gitref
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn number(&self) -> usize {
        self.number
    }

    pub fn title(&self) -> String {
        let title = self.title.trim();
        let title = match &self.draft {
            true => format!("*(Draft) {}*", title),
            false => title.to_owned(),
        };

        match &self.state {
            PullRequestStatus::Open => title,
            PullRequestStatus::Closed => format!("~~{}~~", title),
        }
    }

    pub fn state(&self) -> &PullRequestStatus {
        &self.state
    }

    pub fn review_state(&self) -> PullRequestReviewState {
        if self.merged_at.is_some() {
            PullRequestReviewState::MERGED
        } else if self.at_least_one_approval() {
            PullRequestReviewState::APPROVED
        } else {
            PullRequestReviewState::PENDING
        }
    }

    pub fn body(&self) -> &str {
        match &self.body {
            Some(body) => body,
            None => "",
        }
    }

    pub fn updated_at(&self) -> Option<&str> {
        self.updated_at.as_deref()
    }

    pub fn is_merged(&self) -> bool {
        self.merged_at.is_some()
    }

    pub fn is_draft(&self) -> bool {
        self.draft
    }

    /// Get the SHA of the head commit (for check status lookups)
    pub fn head_sha(&self) -> &str {
        &self.head.sha
    }

    /// Get the raw title without markdown formatting
    pub fn raw_title(&self) -> &str {
        self.title.trim()
    }

    /// Convert API URL to HTML URL for display
    ///
    /// Transforms:
    /// - `https://api.github.com/repos/owner/repo/pulls/123`
    /// - to `https://github.com/owner/repo/pull/123`
    ///
    /// Also handles enterprise URLs:
    /// - `https://api.github.mycompany.com/repos/org/repo/pulls/456`
    /// - to `https://github.mycompany.com/org/repo/pull/456`
    pub fn html_url(&self) -> String {
        // Handle both github.com and enterprise URLs
        // API URLs have "api." prefix and "/repos/" path
        self.url
            .replacen("api.", "", 1)
            .replace("/repos/", "/")
            .replace("/pulls/", "/pull/")
    }

    pub async fn fetch_reviews(
        self,
        credentials: &Credentials,
    ) -> Result<PullRequest, Box<dyn Error>> {
        let reviews = search::fetch_reviews_for_pull_request(&self, credentials).await?;

        let pr = PullRequest { reviews, ..self };

        Ok(pr)
    }

    fn at_least_one_approval(&self) -> bool {
        self.reviews.iter().any(|review| review.is_approved())
    }
}

#[derive(Serialize, Debug)]
struct UpdateDescriptionRequest<'a> {
    body: &'a str,
}

pub async fn update_description(
    description: String,
    pr: Rc<PullRequest>,
    c: &Credentials,
) -> Result<(), Box<dyn Error>> {
    let client = reqwest::Client::new();
    let body = UpdateDescriptionRequest { body: &description };
    let request = api::base_patch_request(&client, c, pr.url()).json(&body);
    request.send().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_head_sha_accessor() {
        let pr = PullRequest::new_for_test(
            123,
            "feature-branch",
            "main",
            "Test PR",
            PullRequestStatus::Open,
            false,
            None,
            vec![],
        );
        // Default test SHA is "abc123"
        assert_eq!(pr.head_sha(), "abc123");
    }

    #[test]
    fn test_html_url_conversion() {
        let pr = PullRequest::new_for_test(
            123,
            "feature-branch",
            "main",
            "Test PR",
            PullRequestStatus::Open,
            false,
            None,
            vec![],
        );
        assert_eq!(pr.html_url(), "https://github.com/test/repo/pull/123");
    }

    #[test]
    fn test_html_url_preserves_enterprise_domain() {
        // Create a PR with enterprise URL
        let pr = PullRequest {
            id: 456,
            number: 456,
            head: PullRequestRef {
                label: "user:feature".to_string(),
                gitref: "feature".to_string(),
                sha: "abc123".to_string(),
            },
            base: PullRequestRef {
                label: "user:main".to_string(),
                gitref: "main".to_string(),
                sha: "def456".to_string(),
            },
            title: "Enterprise PR".to_string(),
            url: "https://api.github.mycompany.com/repos/org/repo/pulls/456".to_string(),
            body: None,
            state: PullRequestStatus::Open,
            merged_at: None,
            updated_at: None,
            draft: false,
            reviews: vec![],
        };
        assert_eq!(
            pr.html_url(),
            "https://github.mycompany.com/org/repo/pull/456"
        );
    }

    #[test]
    fn test_raw_title_trims_whitespace() {
        let pr = PullRequest::new_for_test(
            1,
            "feature",
            "main",
            "  Whitespace Title  ",
            PullRequestStatus::Open,
            false,
            None,
            vec![],
        );
        assert_eq!(pr.raw_title(), "Whitespace Title");
    }

    #[test]
    fn test_raw_title_vs_title_for_draft() {
        let pr = PullRequest::new_for_test(
            1,
            "feature",
            "main",
            "My Feature",
            PullRequestStatus::Open,
            true, // draft
            None,
            vec![],
        );
        // raw_title returns unformatted
        assert_eq!(pr.raw_title(), "My Feature");
        // title() adds draft formatting
        assert_eq!(pr.title(), "*(Draft) My Feature*");
    }
}
