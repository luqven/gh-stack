use std::fs;

use crate::api::{PullRequestReviewState, PullRequestStatus};
use crate::graph::FlatDep;

pub fn build_table(
    deps: &FlatDep,
    title: &str,
    prelude_path: Option<&str>,
    repository: &str,
) -> String {
    let is_complete = deps
        .iter()
        .all(|(node, _)| node.state() == &PullRequestStatus::Closed);

    let mut out = String::new();

    if is_complete {
        out.push_str(&format!("### ✅ Stacked PR Chain: {}\n", title));
    } else {
        out.push_str(&format!("### Stacked PR Chain: {}\n", title));
    }

    if let Some(prelude_path) = prelude_path {
        let prelude = fs::read_to_string(prelude_path).unwrap();
        out.push_str(&prelude);
        out.push('\n');
    }

    out.push_str("| PR | Title | Status |  Merges Into  |\n");
    out.push_str("|:--:|:------|:-------|:-------------:|\n");

    for (node, parent) in deps {
        let review_state = match node.review_state() {
            PullRequestReviewState::APPROVED => {
                format!(
                    "![](https://img.shields.io/github/pulls/detail/state/{}/{}?label={})",
                    repository,
                    &node.number().to_string(),
                    "Approved"
                )
            }
            PullRequestReviewState::MERGED => {
                format!(
                    "![](https://img.shields.io/github/pulls/detail/state/{}/{}?label={})",
                    repository,
                    &node.number().to_string(),
                    "%20"
                )
            }
            PullRequestReviewState::PENDING => {
                format!(
                    "![](https://img.shields.io/github/pulls/detail/state/{}/{}?label={})",
                    repository,
                    &node.number().to_string(),
                    "Pending"
                )
            }
            PullRequestReviewState::CHANGES_REQUESTED => {
                format!(
                    "![](https://img.shields.io/github/pulls/detail/state/{}/{}?label={})",
                    repository,
                    &node.number().to_string(),
                    "Changes Requested"
                )
            }
            PullRequestReviewState::DISMISSED => {
                format!(
                    "![](https://img.shields.io/github/pulls/detail/state/{}/{}?label={})",
                    repository,
                    &node.number().to_string(),
                    "Dismissed"
                )
            }
            PullRequestReviewState::COMMENTED => {
                format!(
                    "![](https://img.shields.io/github/pulls/detail/state/{}/{}?label={})",
                    repository,
                    &node.number().to_string(),
                    "Commented"
                )
            }
        };

        let review_state = if node.review_state() != PullRequestReviewState::MERGED
            && *node.state() == PullRequestStatus::Closed
        {
            format!(
                "![](https://img.shields.io/github/pulls/detail/state/{}/{}?label={})",
                repository,
                &node.number().to_string(),
                "Closed"
            )
        } else {
            review_state
        };

        let row = match (node.state(), parent) {
            (_, None) => format!(
                "|#{}|{}|{}|{}|\n",
                node.number(),
                node.title(),
                review_state,
                "-"
            ),
            (_, Some(parent)) => format!(
                "|#{}|{}|{}|#{}|\n",
                node.number(),
                node.title(),
                review_state,
                parent.number(),
            ),
        };

        out.push_str(&row);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{PullRequest, PullRequestStatus};
    use std::rc::Rc;

    fn make_pr(
        number: usize,
        head: &str,
        base: &str,
        title: &str,
        state: PullRequestStatus,
        draft: bool,
        merged_at: Option<String>,
    ) -> Rc<PullRequest> {
        Rc::new(PullRequest::new_for_test(
            number,
            head,
            base,
            title,
            state,
            draft,
            merged_at,
            vec![],
        ))
    }

    #[test]
    fn test_build_table_single_pr() {
        let pr = make_pr(
            1,
            "feature-1",
            "main",
            "Add new feature",
            PullRequestStatus::Open,
            false,
            None,
        );
        let deps: FlatDep = vec![(pr, None)];

        let table = build_table(&deps, "JIRA-123", None, "user/repo");
        insta::assert_snapshot!(table);
    }

    #[test]
    fn test_build_table_linear_stack() {
        let pr1 = make_pr(
            1,
            "feature-1",
            "main",
            "Base feature",
            PullRequestStatus::Open,
            false,
            None,
        );
        let pr2 = make_pr(
            2,
            "feature-2",
            "feature-1",
            "Second feature",
            PullRequestStatus::Open,
            false,
            None,
        );
        let pr3 = make_pr(
            3,
            "feature-3",
            "feature-2",
            "Third feature",
            PullRequestStatus::Open,
            false,
            None,
        );

        let deps: FlatDep = vec![
            (pr1.clone(), None),
            (pr2.clone(), Some(pr1.clone())),
            (pr3.clone(), Some(pr2.clone())),
        ];

        let table = build_table(&deps, "STACK-456", None, "org/project");
        insta::assert_snapshot!(table);
    }

    #[test]
    fn test_build_table_with_draft_pr() {
        let pr = make_pr(
            1,
            "wip-feature",
            "main",
            "Work in progress",
            PullRequestStatus::Open,
            true,
            None,
        );
        let deps: FlatDep = vec![(pr, None)];

        let table = build_table(&deps, "DRAFT-TEST", None, "user/repo");
        insta::assert_snapshot!(table);
    }

    #[test]
    fn test_build_table_with_closed_pr() {
        let pr = make_pr(
            1,
            "old-feature",
            "main",
            "Completed feature",
            PullRequestStatus::Closed,
            false,
            None,
        );
        let deps: FlatDep = vec![(pr, None)];

        let table = build_table(&deps, "CLOSED-TEST", None, "user/repo");
        insta::assert_snapshot!(table);
    }

    #[test]
    fn test_build_table_with_merged_pr() {
        let pr = make_pr(
            1,
            "merged-feature",
            "main",
            "Merged feature",
            PullRequestStatus::Closed,
            false,
            Some("2024-01-15T10:00:00Z".to_string()),
        );
        let deps: FlatDep = vec![(pr, None)];

        let table = build_table(&deps, "MERGED-TEST", None, "user/repo");
        insta::assert_snapshot!(table);
    }

    #[test]
    fn test_build_table_all_closed_shows_checkmark() {
        let pr1 = make_pr(
            1,
            "feature-1",
            "main",
            "First",
            PullRequestStatus::Closed,
            false,
            None,
        );
        let pr2 = make_pr(
            2,
            "feature-2",
            "feature-1",
            "Second",
            PullRequestStatus::Closed,
            false,
            None,
        );

        let deps: FlatDep = vec![(pr1.clone(), None), (pr2.clone(), Some(pr1.clone()))];

        let table = build_table(&deps, "COMPLETE-STACK", None, "user/repo");
        insta::assert_snapshot!(table);
    }

    #[test]
    fn test_build_table_mixed_states() {
        let pr1 = make_pr(
            1,
            "feature-1",
            "main",
            "Merged base",
            PullRequestStatus::Closed,
            false,
            Some("2024-01-15T10:00:00Z".to_string()),
        );
        let pr2 = make_pr(
            2,
            "feature-2",
            "feature-1",
            "Open follow-up",
            PullRequestStatus::Open,
            false,
            None,
        );
        let pr3 = make_pr(
            3,
            "feature-3",
            "feature-2",
            "Draft WIP",
            PullRequestStatus::Open,
            true,
            None,
        );

        let deps: FlatDep = vec![
            (pr1.clone(), None),
            (pr2.clone(), Some(pr1.clone())),
            (pr3.clone(), Some(pr2.clone())),
        ];

        let table = build_table(&deps, "MIXED-STACK", None, "org/repo");
        insta::assert_snapshot!(table);
    }
}
