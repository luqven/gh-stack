//! Landing logic for stacked PRs
//!
//! This module implements the spr/Graphite optimization pattern:
//! 1. Find the topmost PR where all PRs below it are approved
//! 2. Update that PR's base to the target branch
//! 3. Squash-merge that single PR (contains all commits from the stack)
//! 4. Close all PRs below it with a comment linking to the merged PR

use std::error::Error;
use std::fmt;
use std::rc::Rc;

use crate::api::PullRequest;
use crate::graph::FlatDep;
use crate::Credentials;

/// Represents a plan for landing a stack of PRs
#[derive(Debug)]
pub struct LandPlan {
    /// The PR that will be merged (topmost mergeable PR)
    pub top_pr: Rc<PullRequest>,
    /// PRs below top that will be closed after merge
    pub prs_to_close: Vec<Rc<PullRequest>>,
    /// Target branch to merge into (e.g., "main" or "master")
    pub target_branch: String,
    /// Repository in "owner/repo" format
    pub repository: String,
}

/// Result of a successful landing operation
#[derive(Debug)]
pub struct LandResult {
    /// The PR that was merged
    pub merged_pr: Rc<PullRequest>,
    /// PRs that were closed
    pub closed_prs: Vec<Rc<PullRequest>>,
    /// URL of the merged PR
    pub merge_url: String,
}

/// Errors that can occur during landing
#[derive(Debug)]
pub enum LandError {
    /// No PRs found in the stack
    NoPRsInStack,
    /// No PRs are in a mergeable state
    NoPRsMergeable { reason: String },
    /// A PR is in draft state and blocks landing
    DraftBlocking { pr_number: usize },
    /// A PR requires approval
    ApprovalRequired { pr_number: usize },
    /// API call failed
    ApiError { message: String },
}

impl fmt::Display for LandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LandError::NoPRsInStack => write!(f, "No PRs found in the stack"),
            LandError::NoPRsMergeable { reason } => {
                write!(f, "No PRs are mergeable: {}", reason)
            }
            LandError::DraftBlocking { pr_number } => {
                write!(
                    f,
                    "PR #{} is a draft and blocks landing of PRs above it",
                    pr_number
                )
            }
            LandError::ApprovalRequired { pr_number } => {
                write!(f, "PR #{} requires approval", pr_number)
            }
            LandError::ApiError { message } => write!(f, "API error: {}", message),
        }
    }
}

impl Error for LandError {}

/// Options for creating a land plan
pub struct LandOptions {
    /// Whether to require approval on all PRs
    pub require_approval: bool,
    /// Maximum number of PRs to land (None = all mergeable)
    pub max_count: Option<usize>,
}

impl Default for LandOptions {
    fn default() -> Self {
        LandOptions {
            require_approval: true,
            max_count: None,
        }
    }
}

/// Order the stack from base to top (PRs targeting main/master first)
fn order_stack_base_to_top(stack: &FlatDep) -> Vec<Rc<PullRequest>> {
    // Find root PRs (those with no parent in the stack)
    let mut ordered = Vec::new();
    let mut remaining: Vec<_> = stack.iter().collect();

    // Start with PRs that have no parent (base of stack)
    while !remaining.is_empty() {
        let mut found_any = false;

        remaining.retain(|(pr, parent)| {
            let dominated_by_parent = parent
                .as_ref()
                .map(|p| {
                    // Check if parent is already in ordered list
                    ordered
                        .iter()
                        .any(|o: &Rc<PullRequest>| o.number() == p.number())
                })
                .unwrap_or(true); // No parent means it's a root

            if dominated_by_parent {
                ordered.push(pr.clone());
                found_any = true;
                false // Remove from remaining
            } else {
                true // Keep in remaining
            }
        });

        // Safety: prevent infinite loop if graph is malformed
        if !found_any && !remaining.is_empty() {
            // Add remaining PRs in any order
            for (pr, _) in remaining.drain(..) {
                ordered.push(pr.clone());
            }
        }
    }

    ordered
}

/// Check if a PR is approved (has at least one approval review)
fn is_pr_approved(pr: &PullRequest) -> bool {
    use crate::api::PullRequestReviewState;
    matches!(
        pr.review_state(),
        PullRequestReviewState::APPROVED | PullRequestReviewState::MERGED
    )
}

/// Analyze the stack and create a landing plan
pub fn create_land_plan(
    stack: &FlatDep,
    repository: &str,
    options: &LandOptions,
) -> Result<LandPlan, LandError> {
    if stack.is_empty() {
        return Err(LandError::NoPRsInStack);
    }

    // Order from base to top
    let ordered = order_stack_base_to_top(stack);

    // Filter to only open PRs
    let open_prs: Vec<_> = ordered
        .into_iter()
        .filter(|pr| !pr.is_merged() && pr.state() == &crate::api::PullRequestStatus::Open)
        .collect();

    if open_prs.is_empty() {
        return Err(LandError::NoPRsMergeable {
            reason: "All PRs are already merged or closed".to_string(),
        });
    }

    // Find the target branch (base of the first PR)
    let target_branch = stack
        .iter()
        .find(|(_, parent)| parent.is_none())
        .map(|(pr, _)| pr.base().to_string())
        .unwrap_or_else(|| "main".to_string());

    // Find mergeable PRs (stopping at first draft or unapproved PR)
    let mut mergeable: Vec<Rc<PullRequest>> = Vec::new();

    for pr in open_prs.iter() {
        // Check for draft PRs
        if pr.is_draft() {
            if mergeable.is_empty() {
                return Err(LandError::DraftBlocking {
                    pr_number: pr.number(),
                });
            }
            break; // Draft blocks PRs above it
        }

        // Check for approval if required
        if options.require_approval && !is_pr_approved(pr) {
            if mergeable.is_empty() {
                return Err(LandError::ApprovalRequired {
                    pr_number: pr.number(),
                });
            }
            break; // Unapproved PR blocks PRs above it
        }

        mergeable.push(pr.clone());

        // Respect max_count
        if let Some(max) = options.max_count {
            if mergeable.len() >= max {
                break;
            }
        }
    }

    if mergeable.is_empty() {
        return Err(LandError::NoPRsMergeable {
            reason: "No PRs passed approval/draft checks".to_string(),
        });
    }

    // Top PR = last in mergeable list (will be merged)
    // Rest = PRs to close
    let top_pr = mergeable.pop().unwrap();
    let prs_to_close = mergeable;

    Ok(LandPlan {
        top_pr,
        prs_to_close,
        target_branch,
        repository: repository.to_string(),
    })
}

/// Format the dry-run output for a land plan
pub fn format_dry_run(plan: &LandPlan, remaining_prs: &[Rc<PullRequest>]) -> String {
    let mut output = String::new();

    output.push_str("Landing Plan:\n");
    output.push_str(&format!("  Target branch: {}\n\n", plan.target_branch));

    // PRs to land
    let total_to_land = plan.prs_to_close.len() + 1;
    output.push_str(&format!("  PRs to land ({}):\n", total_to_land));

    for pr in &plan.prs_to_close {
        output.push_str(&format!(
            "    [x] #{}: {} (will close)\n",
            pr.number(),
            pr.title()
        ));
    }
    output.push_str(&format!(
        "    [x] #{}: {} <- will merge\n",
        plan.top_pr.number(),
        plan.top_pr.title()
    ));

    // PRs not included
    if !remaining_prs.is_empty() {
        output.push_str(&format!(
            "\n  PRs not included ({}):\n",
            remaining_prs.len()
        ));
        for pr in remaining_prs {
            let reason = if pr.is_draft() {
                "draft"
            } else {
                "not approved"
            };
            output.push_str(&format!(
                "    [ ] #{}: {} ({})\n",
                pr.number(),
                pr.title(),
                reason
            ));
        }
    }

    output.push_str("\n  Actions that would be taken:\n");
    output.push_str(&format!(
        "    1. Update PR #{} base branch: {} -> {}\n",
        plan.top_pr.number(),
        plan.top_pr.base(),
        plan.target_branch
    ));
    output.push_str(&format!(
        "    2. Squash-merge PR #{} into {}\n",
        plan.top_pr.number(),
        plan.target_branch
    ));

    for (i, pr) in plan.prs_to_close.iter().enumerate() {
        output.push_str(&format!(
            "    {}. Close PR #{} with comment: \"Landed via #{}\"\n",
            i + 3,
            pr.number(),
            plan.top_pr.number()
        ));
    }

    output.push_str("\nRun without --dry-run to execute.\n");

    output
}

/// Execute the landing plan
pub async fn execute_land(
    plan: &LandPlan,
    credentials: &Credentials,
) -> Result<LandResult, LandError> {
    use crate::api::land::{close_pr_with_comment, merge_pr, update_pr_base};

    // Step 1: Update top PR's base to target branch
    println!(
        "  Updating PR #{} base to {}...",
        plan.top_pr.number(),
        plan.target_branch
    );
    update_pr_base(
        plan.top_pr.number(),
        &plan.target_branch,
        &plan.repository,
        credentials,
    )
    .await
    .map_err(|e| LandError::ApiError {
        message: format!("Failed to update PR base: {}", e),
    })?;

    // Step 2: Merge the top PR
    println!("  Merging PR #{}...", plan.top_pr.number());
    let merge_url = merge_pr(plan.top_pr.number(), &plan.repository, credentials)
        .await
        .map_err(|e| LandError::ApiError {
            message: format!("Failed to merge PR: {}", e),
        })?;

    // Step 3: Close all PRs below with comment
    let comment = format!("Landed via #{}", plan.top_pr.number());
    let mut closed_prs = Vec::new();

    for pr in &plan.prs_to_close {
        println!(
            "  Closing PR #{} (landed via #{})...",
            pr.number(),
            plan.top_pr.number()
        );
        close_pr_with_comment(pr.number(), &comment, &plan.repository, credentials)
            .await
            .map_err(|e| LandError::ApiError {
                message: format!("Failed to close PR #{}: {}", pr.number(), e),
            })?;
        closed_prs.push(pr.clone());
    }

    Ok(LandResult {
        merged_pr: plan.top_pr.clone(),
        closed_prs,
        merge_url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{PullRequest, PullRequestStatus};

    fn make_pr(
        number: usize,
        head: &str,
        base: &str,
        approved: bool,
        draft: bool,
    ) -> Rc<PullRequest> {
        let reviews = if approved {
            vec![crate::api::PullRequestReview::new_for_test(
                crate::api::PullRequestReviewState::APPROVED,
            )]
        } else {
            vec![]
        };

        Rc::new(PullRequest::new_for_test(
            number,
            head,
            base,
            &format!("PR #{}", number),
            PullRequestStatus::Open,
            draft,
            None,
            reviews,
        ))
    }

    fn make_stack(prs: Vec<Rc<PullRequest>>) -> FlatDep {
        let mut stack = Vec::new();
        for (i, pr) in prs.iter().enumerate() {
            let parent = if i > 0 {
                Some(prs[i - 1].clone())
            } else {
                None
            };
            stack.push((pr.clone(), parent));
        }
        stack
    }

    #[test]
    fn test_create_plan_empty_stack() {
        let stack: FlatDep = vec![];
        let options = LandOptions::default();
        let result = create_land_plan(&stack, "owner/repo", &options);
        assert!(matches!(result, Err(LandError::NoPRsInStack)));
    }

    #[test]
    fn test_create_plan_single_approved_pr() {
        let pr = make_pr(1, "feature-1", "main", true, false);
        let stack = make_stack(vec![pr.clone()]);
        let options = LandOptions::default();

        let plan = create_land_plan(&stack, "owner/repo", &options).unwrap();

        assert_eq!(plan.top_pr.number(), 1);
        assert!(plan.prs_to_close.is_empty());
        assert_eq!(plan.target_branch, "main");
    }

    #[test]
    fn test_create_plan_all_approved() {
        let prs = vec![
            make_pr(1, "feature-1", "main", true, false),
            make_pr(2, "feature-2", "feature-1", true, false),
            make_pr(3, "feature-3", "feature-2", true, false),
        ];
        let stack = make_stack(prs);
        let options = LandOptions::default();

        let plan = create_land_plan(&stack, "owner/repo", &options).unwrap();

        assert_eq!(plan.top_pr.number(), 3);
        assert_eq!(plan.prs_to_close.len(), 2);
        assert_eq!(plan.prs_to_close[0].number(), 1);
        assert_eq!(plan.prs_to_close[1].number(), 2);
    }

    #[test]
    fn test_create_plan_partial_approval() {
        let prs = vec![
            make_pr(1, "feature-1", "main", true, false),
            make_pr(2, "feature-2", "feature-1", true, false),
            make_pr(3, "feature-3", "feature-2", false, false), // Not approved
        ];
        let stack = make_stack(prs);
        let options = LandOptions::default();

        let plan = create_land_plan(&stack, "owner/repo", &options).unwrap();

        // Should only include the first two approved PRs
        assert_eq!(plan.top_pr.number(), 2);
        assert_eq!(plan.prs_to_close.len(), 1);
        assert_eq!(plan.prs_to_close[0].number(), 1);
    }

    #[test]
    fn test_create_plan_first_pr_not_approved() {
        let prs = vec![
            make_pr(1, "feature-1", "main", false, false), // Not approved
            make_pr(2, "feature-2", "feature-1", true, false),
        ];
        let stack = make_stack(prs);
        let options = LandOptions::default();

        let result = create_land_plan(&stack, "owner/repo", &options);
        assert!(matches!(
            result,
            Err(LandError::ApprovalRequired { pr_number: 1 })
        ));
    }

    #[test]
    fn test_create_plan_draft_blocking() {
        let prs = vec![
            make_pr(1, "feature-1", "main", true, true), // Draft
            make_pr(2, "feature-2", "feature-1", true, false),
        ];
        let stack = make_stack(prs);
        let options = LandOptions::default();

        let result = create_land_plan(&stack, "owner/repo", &options);
        assert!(matches!(
            result,
            Err(LandError::DraftBlocking { pr_number: 1 })
        ));
    }

    #[test]
    fn test_create_plan_with_count() {
        let prs = vec![
            make_pr(1, "feature-1", "main", true, false),
            make_pr(2, "feature-2", "feature-1", true, false),
            make_pr(3, "feature-3", "feature-2", true, false),
        ];
        let stack = make_stack(prs);
        let options = LandOptions {
            require_approval: true,
            max_count: Some(2),
        };

        let plan = create_land_plan(&stack, "owner/repo", &options).unwrap();

        // Should only include first 2 PRs
        assert_eq!(plan.top_pr.number(), 2);
        assert_eq!(plan.prs_to_close.len(), 1);
    }

    #[test]
    fn test_create_plan_no_approval_flag() {
        let prs = vec![
            make_pr(1, "feature-1", "main", false, false), // Not approved
            make_pr(2, "feature-2", "feature-1", false, false), // Not approved
        ];
        let stack = make_stack(prs);
        let options = LandOptions {
            require_approval: false,
            max_count: None,
        };

        let plan = create_land_plan(&stack, "owner/repo", &options).unwrap();

        // Should include all PRs since approval not required
        assert_eq!(plan.top_pr.number(), 2);
        assert_eq!(plan.prs_to_close.len(), 1);
    }

    #[test]
    fn test_order_stack_base_to_top() {
        // Create PRs in reverse order
        let pr3 = make_pr(3, "feature-3", "feature-2", true, false);
        let pr1 = make_pr(1, "feature-1", "main", true, false);
        let pr2 = make_pr(2, "feature-2", "feature-1", true, false);

        let stack: FlatDep = vec![
            (pr3.clone(), Some(pr2.clone())),
            (pr1.clone(), None),
            (pr2.clone(), Some(pr1.clone())),
        ];

        let ordered = order_stack_base_to_top(&stack);

        assert_eq!(ordered[0].number(), 1);
        assert_eq!(ordered[1].number(), 2);
        assert_eq!(ordered[2].number(), 3);
    }
}
