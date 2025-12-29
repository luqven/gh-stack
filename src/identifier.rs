//! Identifier detection and interactive prompts
//!
//! Provides trunk branch detection and interactive stack selection
//! for the smart-default log command.

use crate::api::PullRequest;
use dialoguer::{Input, Select};
use std::error::Error;
use std::process::Command;

/// Common trunk branch names
const TRUNK_BRANCHES: &[&str] = &["main", "master", "develop", "dev", "trunk"];

/// Summary of a stack for display in selection UI
#[derive(Debug, Clone)]
pub struct StackSummary {
    /// The root branch name (first PR's head)
    pub root_branch: String,
    /// Number of PRs in the stack
    pub pr_count: usize,
    /// PR numbers in the stack
    pub pr_numbers: Vec<usize>,
    /// First part of the root PR's title
    pub title_snippet: String,
}

impl StackSummary {
    /// Create a summary from a list of PRs
    ///
    /// PRs should be sorted bottom-to-top (root first)
    pub fn from_prs(prs: &[PullRequest], _trunk: &str) -> Self {
        let root_branch = prs
            .first()
            .map(|pr| pr.head().to_string())
            .unwrap_or_default();

        let pr_numbers: Vec<usize> = prs.iter().map(|pr| pr.number()).collect();

        let title_snippet = prs
            .first()
            .map(|pr| {
                let title = pr.raw_title();
                if title.len() > 40 {
                    format!("{}...", &title[..37])
                } else {
                    title.to_string()
                }
            })
            .unwrap_or_default();

        StackSummary {
            root_branch,
            pr_count: prs.len(),
            pr_numbers,
            title_snippet,
        }
    }

    /// Format for display in selection list
    pub fn display(&self) -> String {
        let prs = self
            .pr_numbers
            .iter()
            .map(|n| format!("#{}", n))
            .collect::<Vec<_>>()
            .join(", ");

        format!(
            "{} ({} PR{}): {}",
            self.root_branch,
            self.pr_count,
            if self.pr_count == 1 { "" } else { "s" },
            prs
        )
    }
}

/// Check if a branch name is a trunk branch
///
/// Returns true if the branch matches the configured trunk or any common trunk name.
pub fn is_trunk_branch(branch: &str, configured_trunk: Option<&str>) -> bool {
    if let Some(trunk) = configured_trunk {
        if branch == trunk {
            return true;
        }
    }

    TRUNK_BRANCHES.contains(&branch)
}

/// Detect the trunk branch from git remote's default branch
///
/// Runs `git remote show origin` to find the HEAD branch.
pub fn detect_trunk_branch() -> Option<String> {
    // Try to get the default branch from the remote
    let output = Command::new("git")
        .args(["remote", "show", "origin"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Look for "HEAD branch: <branch>"
    for line in stdout.lines() {
        let line = line.trim();
        if line.starts_with("HEAD branch:") {
            return line.split(':').nth(1).map(|s| s.trim().to_string());
        }
    }

    None
}

/// Action to take when on trunk branch
#[derive(Debug, Clone, PartialEq)]
pub enum TrunkAction {
    /// User entered an identifier manually
    EnterIdentifier(String),
    /// User selected a stack by index
    SelectStack(usize),
    /// User cancelled
    Cancel,
}

/// Prompt user for action when on trunk branch
///
/// Shows options to enter an identifier or select from detected stacks.
pub fn prompt_trunk_action(stacks: &[StackSummary]) -> Result<TrunkAction, Box<dyn Error>> {
    let mut items = vec!["Enter a stack identifier".to_string()];

    if !stacks.is_empty() {
        items.push(format!(
            "Select from detected stacks ({} found)",
            stacks.len()
        ));
    }

    items.push("Cancel".to_string());

    let selection = Select::new()
        .with_prompt("You're on a trunk branch. What would you like to do?")
        .items(&items)
        .default(0)
        .interact()?;

    match selection {
        0 => {
            // Enter identifier
            let identifier: String = Input::new()
                .with_prompt("Enter stack identifier")
                .interact_text()?;

            if identifier.is_empty() {
                Ok(TrunkAction::Cancel)
            } else {
                Ok(TrunkAction::EnterIdentifier(identifier))
            }
        }
        idx if idx == items.len() - 1 => Ok(TrunkAction::Cancel),
        1 if !stacks.is_empty() => {
            // Select from stacks
            let stack_idx = prompt_select_stack(stacks)?;
            Ok(TrunkAction::SelectStack(stack_idx))
        }
        _ => Ok(TrunkAction::Cancel),
    }
}

/// Prompt user to select a stack from a list
///
/// Returns the index of the selected stack.
pub fn prompt_select_stack(stacks: &[StackSummary]) -> Result<usize, Box<dyn Error>> {
    if stacks.is_empty() {
        return Err("No stacks to select from".into());
    }

    let items: Vec<String> = stacks.iter().map(|s| s.display()).collect();

    let selection = Select::new()
        .with_prompt("Select a stack")
        .items(&items)
        .default(0)
        .interact()?;

    Ok(selection)
}

/// Prompt user to enter an identifier manually
pub fn prompt_identifier() -> Result<String, Box<dyn Error>> {
    let identifier: String = Input::new()
        .with_prompt("Enter stack identifier")
        .interact_text()?;

    Ok(identifier)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_trunk_branch_main() {
        assert!(is_trunk_branch("main", None));
    }

    #[test]
    fn test_is_trunk_branch_master() {
        assert!(is_trunk_branch("master", None));
    }

    #[test]
    fn test_is_trunk_branch_develop() {
        assert!(is_trunk_branch("develop", None));
    }

    #[test]
    fn test_is_trunk_branch_feature_returns_false() {
        assert!(!is_trunk_branch("feat/my-feature", None));
        assert!(!is_trunk_branch("feature-branch", None));
        assert!(!is_trunk_branch("fix/bug", None));
    }

    #[test]
    fn test_is_trunk_branch_configured() {
        // Custom trunk takes precedence
        assert!(is_trunk_branch("production", Some("production")));
        // But common trunks still work
        assert!(is_trunk_branch("main", Some("production")));
    }

    #[test]
    fn test_is_trunk_branch_configured_not_in_common() {
        // A configured trunk that's not in TRUNK_BRANCHES should still match
        assert!(is_trunk_branch("release", Some("release")));
        // But other branches shouldn't
        assert!(!is_trunk_branch("feature", Some("release")));
    }

    #[test]
    fn test_stack_summary_from_prs_single() {
        use crate::api::PullRequestStatus;

        let pr = PullRequest::new_for_test(
            42,
            "feat/my-feature",
            "main",
            "Add awesome feature",
            PullRequestStatus::Open,
            false,
            None,
            vec![],
        );

        let summary = StackSummary::from_prs(&[pr], "main");

        assert_eq!(summary.root_branch, "feat/my-feature");
        assert_eq!(summary.pr_count, 1);
        assert_eq!(summary.pr_numbers, vec![42]);
        assert_eq!(summary.title_snippet, "Add awesome feature");
    }

    #[test]
    fn test_stack_summary_from_prs_multiple() {
        use crate::api::PullRequestStatus;

        let pr1 = PullRequest::new_for_test(
            1,
            "feat/part-1",
            "main",
            "Part 1: Initial setup",
            PullRequestStatus::Open,
            false,
            None,
            vec![],
        );
        let pr2 = PullRequest::new_for_test(
            2,
            "feat/part-2",
            "feat/part-1",
            "Part 2: Implementation",
            PullRequestStatus::Open,
            false,
            None,
            vec![],
        );

        let summary = StackSummary::from_prs(&[pr1, pr2], "main");

        assert_eq!(summary.root_branch, "feat/part-1");
        assert_eq!(summary.pr_count, 2);
        assert_eq!(summary.pr_numbers, vec![1, 2]);
    }

    #[test]
    fn test_stack_summary_truncates_long_title() {
        use crate::api::PullRequestStatus;

        let pr = PullRequest::new_for_test(
            1,
            "feat/long",
            "main",
            "This is a very long title that should be truncated because it exceeds forty characters",
            PullRequestStatus::Open,
            false,
            None,
            vec![],
        );

        let summary = StackSummary::from_prs(&[pr], "main");

        assert!(summary.title_snippet.len() <= 43); // 40 + "..."
        assert!(summary.title_snippet.ends_with("..."));
    }

    #[test]
    fn test_stack_summary_display() {
        let summary = StackSummary {
            root_branch: "feat/my-feature".to_string(),
            pr_count: 2,
            pr_numbers: vec![42, 43],
            title_snippet: "Add feature".to_string(),
        };

        let display = summary.display();
        assert!(display.contains("feat/my-feature"));
        assert!(display.contains("2 PRs"));
        assert!(display.contains("#42"));
        assert!(display.contains("#43"));
    }

    #[test]
    fn test_stack_summary_display_single() {
        let summary = StackSummary {
            root_branch: "feat/single".to_string(),
            pr_count: 1,
            pr_numbers: vec![99],
            title_snippet: "Single PR".to_string(),
        };

        let display = summary.display();
        assert!(display.contains("1 PR)")); // Note: no 's'
    }

    #[test]
    fn test_stack_summary_empty() {
        let summary = StackSummary::from_prs(&[], "main");

        assert!(summary.root_branch.is_empty());
        assert_eq!(summary.pr_count, 0);
        assert!(summary.pr_numbers.is_empty());
    }
}
