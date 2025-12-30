//! Status display logic for PR stacks
//!
//! This module provides functionality to display stack status with CI, approval,
//! merge, and stack health indicators.
//!
//! ## Performance
//!
//! Status checks are fetched in parallel using `futures::join_all` to minimize
//! latency when checking multiple PRs.

use std::path::PathBuf;
use std::rc::Rc;

use futures::future::join_all;
use git2::Repository;
use serde::Serialize;

use crate::api::checks::{fetch_check_status, fetch_mergeable_status, CheckState, CheckStatus};
use crate::api::{PullRequest, PullRequestReviewState};
use crate::graph::FlatDep;
use crate::tree::{
    branch_exists_locally, commits_for_branch, current_branch, format_relative_time,
    parse_timestamp, CommitInfo,
};
use crate::Credentials;

const MAX_TITLE_LEN: usize = 50;
const LEGEND_FILE_NAME: &str = ".gh-stack-legend-seen";

/// Individual status bit result
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StatusBit {
    Passed,
    Failed,
    Pending,
    #[serde(rename = "n/a")]
    NotApplicable,
}

impl StatusBit {
    /// Convert to unicode symbol
    pub fn to_unicode(&self) -> &'static str {
        match self {
            StatusBit::Passed => "✓",
            StatusBit::Failed => "✗",
            StatusBit::Pending => "⏳",
            StatusBit::NotApplicable => "─",
        }
    }

    /// Convert to ASCII symbol
    pub fn to_ascii(&self) -> &'static str {
        match self {
            StatusBit::Passed => "Y",
            StatusBit::Failed => "N",
            StatusBit::Pending => "?",
            StatusBit::NotApplicable => "-",
        }
    }
}

/// Aggregated status for a single PR
#[derive(Debug, Clone, Serialize)]
pub struct PrStatus {
    pub ci: StatusBit,
    pub approved: StatusBit,
    pub mergeable: StatusBit,
    pub stack_clear: StatusBit,
}

impl PrStatus {
    /// Create a status with all bits set to NotApplicable
    pub fn not_applicable() -> Self {
        PrStatus {
            ci: StatusBit::NotApplicable,
            approved: StatusBit::NotApplicable,
            mergeable: StatusBit::NotApplicable,
            stack_clear: StatusBit::NotApplicable,
        }
    }
}

/// Extended entry with status information
#[derive(Debug, Clone, Serialize)]
pub struct StatusEntry {
    pub branch: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr_number: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub is_current: bool,
    pub is_draft: bool,
    pub is_trunk: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<PrStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub commits: Vec<CommitInfo>,
    #[serde(skip_serializing_if = "is_zero")]
    pub extra_commits: usize,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

/// Configuration for status display
#[derive(Debug, Clone)]
pub struct StatusConfig {
    pub use_color: bool,
    pub use_unicode: bool,
    pub show_legend: bool,
    pub include_checks: bool,
    pub json_output: bool,
}

impl Default for StatusConfig {
    fn default() -> Self {
        StatusConfig {
            use_color: true,
            use_unicode: true,
            show_legend: false,
            include_checks: true,
            json_output: false,
        }
    }
}

/// JSON output structure
#[derive(Debug, Serialize)]
pub struct StatusOutput {
    pub stack: Vec<StatusEntry>,
    pub trunk: String,
}

/// Get the path to the legend seen file
fn legend_file_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(LEGEND_FILE_NAME))
}

/// Check if we should show the legend (first run detection)
pub fn should_show_legend() -> bool {
    match legend_file_path() {
        Some(path) if path.exists() => false,
        Some(path) => {
            // Create marker file
            let _ = std::fs::write(&path, "1");
            true
        }
        None => true, // Show if can't determine
    }
}

/// Mark legend as seen (create the marker file)
pub fn mark_legend_seen() {
    if let Some(path) = legend_file_path() {
        let _ = std::fs::write(&path, "1");
    }
}

/// Truncate title to max length with "..."
pub fn truncate_title(title: &str, max_len: usize) -> String {
    if title.chars().count() <= max_len {
        title.to_string()
    } else {
        let truncated: String = title.chars().take(max_len.saturating_sub(3)).collect();
        format!("{}...", truncated)
    }
}

/// Convert CheckStatus to StatusBit
fn check_status_to_bit(status: &CheckStatus) -> StatusBit {
    match status.state {
        CheckState::Success => StatusBit::Passed,
        CheckState::Failure => StatusBit::Failed,
        CheckState::Pending => StatusBit::Pending,
        CheckState::Neutral => StatusBit::NotApplicable,
    }
}

/// Convert approval state to StatusBit
fn approval_to_bit(pr: &PullRequest) -> StatusBit {
    match pr.review_state() {
        PullRequestReviewState::APPROVED | PullRequestReviewState::MERGED => StatusBit::Passed,
        _ => StatusBit::Failed,
    }
}

/// Convert mergeable state to StatusBit
fn mergeable_to_bit(mergeable: Option<bool>) -> StatusBit {
    match mergeable {
        Some(true) => StatusBit::Passed,
        Some(false) => StatusBit::Failed,
        None => StatusBit::Pending,
    }
}

/// Compute stack clear status for a PR at given index
/// A PR is "stack clear" if all PRs below it are approved and not draft
fn compute_stack_clear(entries: &[StatusEntry], index: usize) -> StatusBit {
    // Check all entries below this one (higher indices = lower in stack)
    for entry in entries.iter().skip(index + 1) {
        if entry.is_trunk {
            continue;
        }

        // If any PR below is draft, stack is blocked
        if entry.is_draft {
            return StatusBit::Failed;
        }

        // If any PR below is not approved, stack is blocked
        if let Some(status) = &entry.status {
            if status.approved != StatusBit::Passed {
                return StatusBit::Failed;
            }
        }
    }

    // Also check if this PR itself is approved (can't be stack clear if not approved)
    if let Some(entry) = entries.get(index) {
        if entry.is_draft {
            return StatusBit::Failed;
        }
        if let Some(status) = &entry.status {
            if status.approved != StatusBit::Passed {
                return StatusBit::Failed;
            }
        }
    }

    StatusBit::Passed
}

/// Intermediate data for building status entries
struct PrCheckData {
    pr: Rc<PullRequest>,
    is_current: bool,
    commits: Vec<CommitInfo>,
    extra_commits: usize,
}

/// Fetch CI and mergeable status for a single PR
async fn fetch_pr_status(
    pr: &PullRequest,
    repository: &str,
    credentials: &Credentials,
) -> (StatusBit, StatusBit) {
    // Fetch CI status and mergeable status in parallel
    let (ci_result, mergeable_result) = futures::join!(
        fetch_check_status(pr.head_sha(), repository, credentials),
        fetch_mergeable_status(pr.number(), repository, credentials)
    );

    let ci = match ci_result {
        Ok(check) => check_status_to_bit(&check),
        Err(_) => StatusBit::NotApplicable,
    };

    let mergeable = match mergeable_result {
        Ok(m) => mergeable_to_bit(m),
        Err(_) => StatusBit::NotApplicable,
    };

    (ci, mergeable)
}

/// Build status entries from a PR stack
///
/// Fetches CI and mergeable status for all PRs in parallel for better performance.
pub async fn build_status_entries(
    stack: &FlatDep,
    repo: Option<&Repository>,
    repository: &str,
    credentials: &Credentials,
    config: &StatusConfig,
) -> Vec<StatusEntry> {
    let current = repo.and_then(current_branch);

    // Get trunk branch from first PR's base
    let trunk_branch = stack.first().map(|(pr, _)| pr.base().to_string());

    // Collect PR data (non-async operations)
    let pr_data: Vec<PrCheckData> = stack
        .iter()
        .rev()
        .filter(|(pr, _)| !pr.is_merged() && pr.state() != &crate::api::PullRequestStatus::Closed)
        .map(|(pr, _)| {
            let is_current = current.as_ref().is_some_and(|c| c == pr.head());

            // Get commits if we have a repo
            let (commits, extra_commits) = if let Some(r) = repo {
                if branch_exists_locally(r, pr.head()) {
                    commits_for_branch(r, pr.head(), pr.base())
                } else {
                    (vec![], 0)
                }
            } else {
                (vec![], 0)
            };

            PrCheckData {
                pr: pr.clone(),
                is_current,
                commits,
                extra_commits,
            }
        })
        .collect();

    // Fetch status checks in parallel if enabled
    let statuses: Vec<Option<(StatusBit, StatusBit)>> = if config.include_checks {
        let futures: Vec<_> = pr_data
            .iter()
            .map(|data| fetch_pr_status(&data.pr, repository, credentials))
            .collect();

        join_all(futures).await.into_iter().map(Some).collect()
    } else {
        vec![None; pr_data.len()]
    };

    // Build entries from collected data
    let mut entries: Vec<StatusEntry> = pr_data
        .into_iter()
        .zip(statuses)
        .map(|(data, status_bits)| {
            let timestamp = data.pr.updated_at().and_then(parse_timestamp);

            let status = status_bits.map(|(ci, mergeable)| PrStatus {
                ci,
                approved: approval_to_bit(&data.pr),
                mergeable,
                stack_clear: StatusBit::Pending, // Will be computed after all entries are built
            });

            StatusEntry {
                branch: data.pr.head().to_string(),
                pr_number: Some(data.pr.number()),
                title: Some(truncate_title(data.pr.raw_title(), MAX_TITLE_LEN)),
                is_current: data.is_current,
                is_draft: data.pr.is_draft(),
                is_trunk: false,
                status,
                updated_at: timestamp.map(|t| t.to_rfc3339()),
                commits: data.commits,
                extra_commits: data.extra_commits,
            }
        })
        .collect();

    // Compute stack_clear for each entry (requires all entries to be built first)
    if config.include_checks {
        for i in 0..entries.len() {
            let stack_clear = compute_stack_clear(&entries, i);
            if let Some(status) = &mut entries[i].status {
                status.stack_clear = stack_clear;
            }
        }
    }

    // Add trunk branch as final entry
    if let Some(trunk) = trunk_branch {
        let is_current = current.as_ref() == Some(&trunk);

        entries.push(StatusEntry {
            branch: trunk,
            pr_number: None,
            title: None,
            is_current,
            is_draft: false,
            is_trunk: true,
            status: None,
            updated_at: None,
            commits: vec![],
            extra_commits: 0,
        });
    }

    entries
}

/// Format status bits for display
pub fn format_status_bits(status: &PrStatus, use_unicode: bool) -> String {
    let bits = [
        status.ci,
        status.approved,
        status.mergeable,
        status.stack_clear,
    ];

    let symbols: Vec<&str> = bits
        .iter()
        .map(|b| {
            if use_unicode {
                b.to_unicode()
            } else {
                b.to_ascii()
            }
        })
        .collect();

    format!(
        "[{} {} {} {}]",
        symbols[0], symbols[1], symbols[2], symbols[3]
    )
}

/// Format the legend text
pub fn format_legend(use_unicode: bool) -> String {
    let mut out = String::new();
    out.push_str("\nStatus: [CI | Approved | Mergeable | Stack]\n");

    if use_unicode {
        out.push_str("  ✓ pass  ✗ fail  ⏳ pending  ─ n/a\n");
    } else {
        out.push_str("  Y=pass  N=fail  ?=pending  -=n/a\n");
    }

    out
}

/// Render status entries to string
pub fn render_status(entries: &[StatusEntry], config: &StatusConfig, has_repo: bool) -> String {
    use console::style;

    let mut out = String::new();

    // Symbols
    let (current_node, other_node, pipe) = if config.use_unicode {
        ("\u{25C9}", "\u{25EF}", "\u{2502}")
    } else {
        ("*", "o", "|")
    };

    for (i, entry) in entries.iter().enumerate() {
        let is_last = i == entries.len() - 1;

        // Node symbol
        let node = if entry.is_current {
            if config.use_color {
                style(current_node).green().bold().to_string()
            } else {
                current_node.to_string()
            }
        } else if config.use_color {
            style(other_node).dim().to_string()
        } else {
            other_node.to_string()
        };

        // Branch name with optional annotations
        let mut branch_display = entry.branch.clone();
        if entry.is_current {
            branch_display = format!("{} (current)", branch_display);
        }

        // Add PR number and title if available
        if let Some(pr_num) = entry.pr_number {
            if let Some(title) = &entry.title {
                branch_display = format!("{} #{} - {}", branch_display, pr_num, title);
            } else {
                branch_display = format!("{} #{}", branch_display, pr_num);
            }
        }

        // Add draft indicator
        if entry.is_draft {
            branch_display = format!("{} (draft)", branch_display);
        }

        out.push_str(&format!("{} {}\n", node, branch_display));

        // Connector for content below
        let connector = if is_last { " " } else { pipe };

        // Status bits (if available)
        if let Some(status) = &entry.status {
            let bits = format_status_bits(status, config.use_unicode);
            let styled_bits = if config.use_color {
                colorize_status_bits(status, config.use_unicode)
            } else {
                bits
            };

            // Add timestamp on same line as status
            if let Some(updated_at) = &entry.updated_at {
                if let Some(ts) = parse_timestamp(updated_at) {
                    let time_str = format_relative_time(&ts);
                    let styled_time = if config.use_color {
                        style(&time_str).dim().to_string()
                    } else {
                        time_str
                    };
                    out.push_str(&format!("{} {}  {}\n", connector, styled_bits, styled_time));
                } else {
                    out.push_str(&format!("{} {}\n", connector, styled_bits));
                }
            } else {
                out.push_str(&format!("{} {}\n", connector, styled_bits));
            }
        } else if let Some(updated_at) = &entry.updated_at {
            // No status bits, just timestamp
            if let Some(ts) = parse_timestamp(updated_at) {
                let time_str = format_relative_time(&ts);
                let styled_time = if config.use_color {
                    style(&time_str).dim().to_string()
                } else {
                    time_str
                };
                out.push_str(&format!("{} {}\n", connector, styled_time));
            }
        }

        // Commits (only for non-trunk entries with commits)
        if !entry.commits.is_empty() {
            out.push_str(&format!("{}\n", connector));
            for commit in &entry.commits {
                let commit_line = format!("{} - {}", commit.sha, commit.message);
                let styled_commit = if config.use_color {
                    style(&commit_line).dim().to_string()
                } else {
                    commit_line
                };
                out.push_str(&format!("{} {}\n", connector, styled_commit));
            }

            // Show "+ N more" if there are extra commits
            if entry.extra_commits > 0 {
                let more_text = format!("+ {} more", entry.extra_commits);
                let styled_more = if config.use_color {
                    style(&more_text).dim().to_string()
                } else {
                    more_text
                };
                out.push_str(&format!("{} {}\n", connector, styled_more));
            }
        }

        // Empty line before next entry (except last)
        if !is_last {
            out.push_str(&format!("{}\n", pipe));
        }
    }

    // Show legend if configured
    if config.show_legend {
        out.push_str(&format_legend(config.use_unicode));
    }

    // Hint if no repo detected
    if !has_repo && !entries.is_empty() {
        out.push_str(
            "\nhint: run from a git repo or use -C <path> to see commits and current branch\n",
        );
    }

    out
}

/// Colorize status bits with appropriate colors
fn colorize_status_bits(status: &PrStatus, use_unicode: bool) -> String {
    use console::style;

    let colorize = |bit: StatusBit| -> String {
        let symbol = if use_unicode {
            bit.to_unicode()
        } else {
            bit.to_ascii()
        };

        match bit {
            StatusBit::Passed => style(symbol).green().to_string(),
            StatusBit::Failed => style(symbol).red().to_string(),
            StatusBit::Pending => style(symbol).yellow().to_string(),
            StatusBit::NotApplicable => style(symbol).dim().to_string(),
        }
    };

    format!(
        "[{} {} {} {}]",
        colorize(status.ci),
        colorize(status.approved),
        colorize(status.mergeable),
        colorize(status.stack_clear)
    )
}

/// Render status entries as JSON
pub fn render_status_json(entries: &[StatusEntry]) -> Result<String, serde_json::Error> {
    let trunk = entries
        .iter()
        .find(|e| e.is_trunk)
        .map(|e| e.branch.clone())
        .unwrap_or_else(|| "main".to_string());

    let stack: Vec<StatusEntry> = entries.iter().filter(|e| !e.is_trunk).cloned().collect();

    let output = StatusOutput { stack, trunk };
    serde_json::to_string_pretty(&output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{PullRequest, PullRequestStatus};
    use serial_test::serial;
    use tempfile::TempDir;

    // === StatusBit tests ===

    #[test]
    fn test_status_bit_to_unicode() {
        assert_eq!(StatusBit::Passed.to_unicode(), "✓");
        assert_eq!(StatusBit::Failed.to_unicode(), "✗");
        assert_eq!(StatusBit::Pending.to_unicode(), "⏳");
        assert_eq!(StatusBit::NotApplicable.to_unicode(), "─");
    }

    #[test]
    fn test_status_bit_to_ascii() {
        assert_eq!(StatusBit::Passed.to_ascii(), "Y");
        assert_eq!(StatusBit::Failed.to_ascii(), "N");
        assert_eq!(StatusBit::Pending.to_ascii(), "?");
        assert_eq!(StatusBit::NotApplicable.to_ascii(), "-");
    }

    // === CheckStatus to StatusBit tests ===

    #[test]
    fn test_check_status_to_bit_success() {
        let status = CheckStatus {
            state: CheckState::Success,
            total: 1,
            passed: 1,
            failed: 0,
            pending: 0,
        };
        assert_eq!(check_status_to_bit(&status), StatusBit::Passed);
    }

    #[test]
    fn test_check_status_to_bit_failure() {
        let status = CheckStatus {
            state: CheckState::Failure,
            total: 1,
            passed: 0,
            failed: 1,
            pending: 0,
        };
        assert_eq!(check_status_to_bit(&status), StatusBit::Failed);
    }

    #[test]
    fn test_check_status_to_bit_pending() {
        let status = CheckStatus {
            state: CheckState::Pending,
            total: 1,
            passed: 0,
            failed: 0,
            pending: 1,
        };
        assert_eq!(check_status_to_bit(&status), StatusBit::Pending);
    }

    #[test]
    fn test_check_status_to_bit_neutral() {
        let status = CheckStatus {
            state: CheckState::Neutral,
            total: 0,
            passed: 0,
            failed: 0,
            pending: 0,
        };
        assert_eq!(check_status_to_bit(&status), StatusBit::NotApplicable);
    }

    // === Approval to StatusBit tests ===

    #[test]
    fn test_approval_to_bit_approved() {
        let pr = PullRequest::new_for_test(
            1,
            "feature",
            "main",
            "Test PR",
            PullRequestStatus::Open,
            false,
            None,
            vec![crate::api::PullRequestReview::new_for_test(
                PullRequestReviewState::APPROVED,
            )],
        );
        assert_eq!(approval_to_bit(&pr), StatusBit::Passed);
    }

    #[test]
    fn test_approval_to_bit_pending() {
        let pr = PullRequest::new_for_test(
            1,
            "feature",
            "main",
            "Test PR",
            PullRequestStatus::Open,
            false,
            None,
            vec![],
        );
        assert_eq!(approval_to_bit(&pr), StatusBit::Failed);
    }

    // === Mergeable to StatusBit tests ===

    #[test]
    fn test_mergeable_to_bit_true() {
        assert_eq!(mergeable_to_bit(Some(true)), StatusBit::Passed);
    }

    #[test]
    fn test_mergeable_to_bit_false() {
        assert_eq!(mergeable_to_bit(Some(false)), StatusBit::Failed);
    }

    #[test]
    fn test_mergeable_to_bit_unknown() {
        assert_eq!(mergeable_to_bit(None), StatusBit::Pending);
    }

    // === Truncate title tests ===

    #[test]
    fn test_truncate_title_short() {
        assert_eq!(truncate_title("Short title", 50), "Short title");
    }

    #[test]
    fn test_truncate_title_exact() {
        let title = "x".repeat(50);
        assert_eq!(truncate_title(&title, 50), title);
    }

    #[test]
    fn test_truncate_title_long() {
        let title = "x".repeat(60);
        let result = truncate_title(&title, 50);
        assert_eq!(result.len(), 50);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_truncate_title_unicode() {
        let title = "Add 日本語 support for the feature system here";
        let result = truncate_title(title, 20);
        assert!(result.chars().count() <= 20);
        assert!(result.ends_with("..."));
    }

    // === Format status bits tests ===

    #[test]
    fn test_format_status_bits_unicode_all_passed() {
        let status = PrStatus {
            ci: StatusBit::Passed,
            approved: StatusBit::Passed,
            mergeable: StatusBit::Passed,
            stack_clear: StatusBit::Passed,
        };
        assert_eq!(format_status_bits(&status, true), "[✓ ✓ ✓ ✓]");
    }

    #[test]
    fn test_format_status_bits_unicode_mixed() {
        let status = PrStatus {
            ci: StatusBit::Pending,
            approved: StatusBit::Failed,
            mergeable: StatusBit::Passed,
            stack_clear: StatusBit::Failed,
        };
        assert_eq!(format_status_bits(&status, true), "[⏳ ✗ ✓ ✗]");
    }

    #[test]
    fn test_format_status_bits_ascii_all_passed() {
        let status = PrStatus {
            ci: StatusBit::Passed,
            approved: StatusBit::Passed,
            mergeable: StatusBit::Passed,
            stack_clear: StatusBit::Passed,
        };
        assert_eq!(format_status_bits(&status, false), "[Y Y Y Y]");
    }

    #[test]
    fn test_format_status_bits_ascii_mixed() {
        let status = PrStatus {
            ci: StatusBit::Pending,
            approved: StatusBit::Failed,
            mergeable: StatusBit::Passed,
            stack_clear: StatusBit::Failed,
        };
        assert_eq!(format_status_bits(&status, false), "[? N Y N]");
    }

    #[test]
    fn test_format_status_bits_with_na() {
        let status = PrStatus {
            ci: StatusBit::Passed,
            approved: StatusBit::NotApplicable,
            mergeable: StatusBit::Passed,
            stack_clear: StatusBit::Passed,
        };
        assert_eq!(format_status_bits(&status, true), "[✓ ─ ✓ ✓]");
        assert_eq!(format_status_bits(&status, false), "[Y - Y Y]");
    }

    // === Stack clear computation tests ===

    fn make_status_entry(
        branch: &str,
        is_draft: bool,
        is_trunk: bool,
        approved: StatusBit,
    ) -> StatusEntry {
        StatusEntry {
            branch: branch.to_string(),
            pr_number: Some(1),
            title: Some("Test".to_string()),
            is_current: false,
            is_draft,
            is_trunk,
            status: Some(PrStatus {
                ci: StatusBit::Passed,
                approved,
                mergeable: StatusBit::Passed,
                stack_clear: StatusBit::Pending,
            }),
            updated_at: None,
            commits: vec![],
            extra_commits: 0,
        }
    }

    #[test]
    fn test_compute_stack_clear_all_approved() {
        let entries = vec![
            make_status_entry("feature-3", false, false, StatusBit::Passed),
            make_status_entry("feature-2", false, false, StatusBit::Passed),
            make_status_entry("feature-1", false, false, StatusBit::Passed),
            StatusEntry {
                branch: "main".to_string(),
                pr_number: None,
                title: None,
                is_current: false,
                is_draft: false,
                is_trunk: true,
                status: None,
                updated_at: None,
                commits: vec![],
                extra_commits: 0,
            },
        ];

        assert_eq!(compute_stack_clear(&entries, 0), StatusBit::Passed);
        assert_eq!(compute_stack_clear(&entries, 1), StatusBit::Passed);
        assert_eq!(compute_stack_clear(&entries, 2), StatusBit::Passed);
    }

    #[test]
    fn test_compute_stack_clear_blocked_by_draft() {
        let entries = vec![
            make_status_entry("feature-2", false, false, StatusBit::Passed),
            make_status_entry("feature-1", true, false, StatusBit::Passed), // draft
            StatusEntry {
                branch: "main".to_string(),
                pr_number: None,
                title: None,
                is_current: false,
                is_draft: false,
                is_trunk: true,
                status: None,
                updated_at: None,
                commits: vec![],
                extra_commits: 0,
            },
        ];

        assert_eq!(compute_stack_clear(&entries, 0), StatusBit::Failed); // blocked by draft below
        assert_eq!(compute_stack_clear(&entries, 1), StatusBit::Failed); // is draft
    }

    #[test]
    fn test_compute_stack_clear_blocked_by_unapproved() {
        let entries = vec![
            make_status_entry("feature-2", false, false, StatusBit::Passed),
            make_status_entry("feature-1", false, false, StatusBit::Failed), // not approved
            StatusEntry {
                branch: "main".to_string(),
                pr_number: None,
                title: None,
                is_current: false,
                is_draft: false,
                is_trunk: true,
                status: None,
                updated_at: None,
                commits: vec![],
                extra_commits: 0,
            },
        ];

        assert_eq!(compute_stack_clear(&entries, 0), StatusBit::Failed); // blocked
        assert_eq!(compute_stack_clear(&entries, 1), StatusBit::Failed); // not approved
    }

    #[test]
    fn test_compute_stack_clear_single_pr() {
        let entries = vec![
            make_status_entry("feature-1", false, false, StatusBit::Passed),
            StatusEntry {
                branch: "main".to_string(),
                pr_number: None,
                title: None,
                is_current: false,
                is_draft: false,
                is_trunk: true,
                status: None,
                updated_at: None,
                commits: vec![],
                extra_commits: 0,
            },
        ];

        assert_eq!(compute_stack_clear(&entries, 0), StatusBit::Passed);
    }

    // === Legend file tests with temp directory ===

    fn with_temp_home<F>(test_fn: F)
    where
        F: FnOnce(&std::path::Path),
    {
        let temp_dir = TempDir::new().unwrap();
        let original_home = std::env::var("HOME").ok();

        // Override HOME for test
        std::env::set_var("HOME", temp_dir.path());

        test_fn(temp_dir.path());

        // Restore original HOME
        if let Some(home) = original_home {
            std::env::set_var("HOME", home);
        } else {
            std::env::remove_var("HOME");
        }
        // TempDir automatically cleaned up on drop
    }

    #[test]
    #[serial]
    fn test_should_show_legend_first_run() {
        with_temp_home(|_| {
            // First call should return true and create the file
            assert!(should_show_legend());
        });
    }

    #[test]
    #[serial]
    fn test_should_show_legend_subsequent_run() {
        with_temp_home(|home| {
            // Create the legend file
            let legend_path = home.join(LEGEND_FILE_NAME);
            std::fs::write(&legend_path, "1").unwrap();

            // Should return false now
            assert!(!should_show_legend());
        });
    }

    // === JSON output tests ===

    #[test]
    fn test_json_output_structure() {
        let entries = vec![
            StatusEntry {
                branch: "feature".to_string(),
                pr_number: Some(123),
                title: Some("Test PR".to_string()),
                is_current: true,
                is_draft: false,
                is_trunk: false,
                status: Some(PrStatus {
                    ci: StatusBit::Passed,
                    approved: StatusBit::Passed,
                    mergeable: StatusBit::Passed,
                    stack_clear: StatusBit::Passed,
                }),
                updated_at: None,
                commits: vec![],
                extra_commits: 0,
            },
            StatusEntry {
                branch: "main".to_string(),
                pr_number: None,
                title: None,
                is_current: false,
                is_draft: false,
                is_trunk: true,
                status: None,
                updated_at: None,
                commits: vec![],
                extra_commits: 0,
            },
        ];

        let json = render_status_json(&entries).unwrap();
        assert!(json.contains("\"trunk\": \"main\""));
        assert!(json.contains("\"branch\": \"feature\""));
        assert!(json.contains("\"pr_number\": 123"));
    }

    #[test]
    fn test_json_output_status_values() {
        let entries = vec![StatusEntry {
            branch: "feature".to_string(),
            pr_number: Some(1),
            title: Some("Test".to_string()),
            is_current: false,
            is_draft: false,
            is_trunk: false,
            status: Some(PrStatus {
                ci: StatusBit::Passed,
                approved: StatusBit::Failed,
                mergeable: StatusBit::Pending,
                stack_clear: StatusBit::NotApplicable,
            }),
            updated_at: None,
            commits: vec![],
            extra_commits: 0,
        }];

        let json = render_status_json(&entries).unwrap();
        assert!(json.contains("\"ci\": \"passed\""));
        assert!(json.contains("\"approved\": \"failed\""));
        assert!(json.contains("\"mergeable\": \"pending\""));
        assert!(json.contains("\"stack_clear\": \"n/a\""));
    }

    #[test]
    fn test_json_output_pretty_formatted() {
        let entries = vec![StatusEntry {
            branch: "feature".to_string(),
            pr_number: Some(1),
            title: Some("Test".to_string()),
            is_current: false,
            is_draft: false,
            is_trunk: false,
            status: None,
            updated_at: None,
            commits: vec![],
            extra_commits: 0,
        }];

        let json = render_status_json(&entries).unwrap();
        // Pretty-printed JSON has newlines
        assert!(json.contains('\n'));
    }

    // === Snapshot tests ===

    fn make_test_entry(
        branch: &str,
        pr_number: Option<usize>,
        title: Option<&str>,
        is_current: bool,
        is_draft: bool,
        is_trunk: bool,
        status: Option<PrStatus>,
    ) -> StatusEntry {
        StatusEntry {
            branch: branch.to_string(),
            pr_number,
            title: title.map(String::from),
            is_current,
            is_draft,
            is_trunk,
            status,
            updated_at: None,
            commits: vec![],
            extra_commits: 0,
        }
    }

    #[test]
    fn test_snapshot_status_all_passing() {
        let config = StatusConfig {
            use_color: false,
            use_unicode: false,
            show_legend: false,
            include_checks: true,
            json_output: false,
        };

        let entries = vec![
            make_test_entry(
                "feature-2",
                Some(124),
                Some("Add new feature"),
                true,
                false,
                false,
                Some(PrStatus {
                    ci: StatusBit::Passed,
                    approved: StatusBit::Passed,
                    mergeable: StatusBit::Passed,
                    stack_clear: StatusBit::Passed,
                }),
            ),
            make_test_entry(
                "feature-1",
                Some(123),
                Some("Setup base"),
                false,
                false,
                false,
                Some(PrStatus {
                    ci: StatusBit::Passed,
                    approved: StatusBit::Passed,
                    mergeable: StatusBit::Passed,
                    stack_clear: StatusBit::Passed,
                }),
            ),
            make_test_entry("main", None, None, false, false, true, None),
        ];

        let output = render_status(&entries, &config, true);
        insta::assert_snapshot!(output);
    }

    #[test]
    fn test_snapshot_status_mixed() {
        let config = StatusConfig {
            use_color: false,
            use_unicode: false,
            show_legend: false,
            include_checks: true,
            json_output: false,
        };

        let entries = vec![
            make_test_entry(
                "feature-2",
                Some(124),
                Some("Add new feature"),
                true,
                false,
                false,
                Some(PrStatus {
                    ci: StatusBit::Pending,
                    approved: StatusBit::Failed,
                    mergeable: StatusBit::Passed,
                    stack_clear: StatusBit::Failed,
                }),
            ),
            make_test_entry(
                "feature-1",
                Some(123),
                Some("Setup base"),
                false,
                false,
                false,
                Some(PrStatus {
                    ci: StatusBit::Passed,
                    approved: StatusBit::Passed,
                    mergeable: StatusBit::Failed,
                    stack_clear: StatusBit::Passed,
                }),
            ),
            make_test_entry("main", None, None, false, false, true, None),
        ];

        let output = render_status(&entries, &config, true);
        insta::assert_snapshot!(output);
    }

    #[test]
    fn test_snapshot_status_with_draft() {
        let config = StatusConfig {
            use_color: false,
            use_unicode: false,
            show_legend: false,
            include_checks: true,
            json_output: false,
        };

        let entries = vec![
            make_test_entry(
                "wip-feature",
                Some(125),
                Some("Work in progress"),
                true,
                true, // draft
                false,
                Some(PrStatus {
                    ci: StatusBit::Pending,
                    approved: StatusBit::Failed,
                    mergeable: StatusBit::Passed,
                    stack_clear: StatusBit::Failed,
                }),
            ),
            make_test_entry(
                "feature-1",
                Some(123),
                Some("Setup base"),
                false,
                false,
                false,
                Some(PrStatus {
                    ci: StatusBit::Passed,
                    approved: StatusBit::Passed,
                    mergeable: StatusBit::Passed,
                    stack_clear: StatusBit::Passed,
                }),
            ),
            make_test_entry("main", None, None, false, false, true, None),
        ];

        let output = render_status(&entries, &config, true);
        insta::assert_snapshot!(output);
    }

    #[test]
    fn test_snapshot_status_no_checks() {
        let config = StatusConfig {
            use_color: false,
            use_unicode: false,
            show_legend: false,
            include_checks: false, // no checks
            json_output: false,
        };

        let entries = vec![
            make_test_entry(
                "feature-2",
                Some(124),
                Some("Add new feature"),
                true,
                false,
                false,
                None, // no status
            ),
            make_test_entry(
                "feature-1",
                Some(123),
                Some("Setup base"),
                false,
                false,
                false,
                None,
            ),
            make_test_entry("main", None, None, false, false, true, None),
        ];

        let output = render_status(&entries, &config, true);
        insta::assert_snapshot!(output);
    }

    #[test]
    fn test_snapshot_status_with_commits() {
        let config = StatusConfig {
            use_color: false,
            use_unicode: false,
            show_legend: false,
            include_checks: true,
            json_output: false,
        };

        let entries = vec![
            StatusEntry {
                branch: "feature-1".to_string(),
                pr_number: Some(123),
                title: Some("Add feature".to_string()),
                is_current: true,
                is_draft: false,
                is_trunk: false,
                status: Some(PrStatus {
                    ci: StatusBit::Passed,
                    approved: StatusBit::Passed,
                    mergeable: StatusBit::Passed,
                    stack_clear: StatusBit::Passed,
                }),
                updated_at: None,
                commits: vec![
                    CommitInfo {
                        sha: "abc1234".to_string(),
                        message: "Add widget component".to_string(),
                    },
                    CommitInfo {
                        sha: "def5678".to_string(),
                        message: "Update styles".to_string(),
                    },
                ],
                extra_commits: 2,
            },
            make_test_entry("main", None, None, false, false, true, None),
        ];

        let output = render_status(&entries, &config, true);
        insta::assert_snapshot!(output);
    }

    #[test]
    fn test_snapshot_status_with_legend() {
        let config = StatusConfig {
            use_color: false,
            use_unicode: false,
            show_legend: true, // show legend
            include_checks: true,
            json_output: false,
        };

        let entries = vec![
            make_test_entry(
                "feature-1",
                Some(123),
                Some("Add feature"),
                false,
                false,
                false,
                Some(PrStatus {
                    ci: StatusBit::Passed,
                    approved: StatusBit::Passed,
                    mergeable: StatusBit::Passed,
                    stack_clear: StatusBit::Passed,
                }),
            ),
            make_test_entry("main", None, None, false, false, true, None),
        ];

        let output = render_status(&entries, &config, true);
        insta::assert_snapshot!(output);
    }

    #[test]
    fn test_snapshot_status_unicode() {
        let config = StatusConfig {
            use_color: false,
            use_unicode: true, // unicode
            show_legend: false,
            include_checks: true,
            json_output: false,
        };

        let entries = vec![
            make_test_entry(
                "feature-1",
                Some(123),
                Some("Add feature"),
                true,
                false,
                false,
                Some(PrStatus {
                    ci: StatusBit::Passed,
                    approved: StatusBit::Failed,
                    mergeable: StatusBit::Pending,
                    stack_clear: StatusBit::NotApplicable,
                }),
            ),
            make_test_entry("main", None, None, false, false, true, None),
        ];

        let output = render_status(&entries, &config, true);
        insta::assert_snapshot!(output);
    }

    #[test]
    fn test_snapshot_status_json() {
        let entries = vec![
            StatusEntry {
                branch: "feature-1".to_string(),
                pr_number: Some(123),
                title: Some("Add feature".to_string()),
                is_current: true,
                is_draft: false,
                is_trunk: false,
                status: Some(PrStatus {
                    ci: StatusBit::Passed,
                    approved: StatusBit::Passed,
                    mergeable: StatusBit::Passed,
                    stack_clear: StatusBit::Passed,
                }),
                updated_at: Some("2024-01-15T10:30:00Z".to_string()),
                commits: vec![CommitInfo {
                    sha: "abc1234".to_string(),
                    message: "Add widget".to_string(),
                }],
                extra_commits: 0,
            },
            StatusEntry {
                branch: "main".to_string(),
                pr_number: None,
                title: None,
                is_current: false,
                is_draft: false,
                is_trunk: true,
                status: None,
                updated_at: None,
                commits: vec![],
                extra_commits: 0,
            },
        ];

        let output = render_status_json(&entries).unwrap();
        insta::assert_snapshot!(output);
    }

    #[test]
    fn test_snapshot_status_single_pr() {
        let config = StatusConfig {
            use_color: false,
            use_unicode: false,
            show_legend: false,
            include_checks: true,
            json_output: false,
        };

        let entries = vec![
            make_test_entry(
                "feature-1",
                Some(123),
                Some("Single PR"),
                false,
                false,
                false,
                Some(PrStatus {
                    ci: StatusBit::Passed,
                    approved: StatusBit::Passed,
                    mergeable: StatusBit::Passed,
                    stack_clear: StatusBit::Passed,
                }),
            ),
            make_test_entry("main", None, None, false, false, true, None),
        ];

        let output = render_status(&entries, &config, true);
        insta::assert_snapshot!(output);
    }
}
