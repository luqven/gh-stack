// src/tree.rs
//! Tree rendering logic for visualizing PR stacks

use crate::api::pull_request::PullRequestStatus;
use crate::api::PullRequest;
use crate::graph::FlatDep;
use chrono::{DateTime, Utc};
use console::style;
use git2::{Repository, Sort};
use std::io::IsTerminal;
use std::rc::Rc;

const MAX_COMMITS: usize = 3;
const MAX_MESSAGE_LEN: usize = 60;

/// Configuration for tree rendering
pub struct TreeConfig {
    pub use_color: bool,
    pub use_unicode: bool,
    pub include_closed: bool,
}

impl TreeConfig {
    /// Detect appropriate config based on terminal capabilities
    pub fn detect(no_color_flag: bool) -> Self {
        let is_tty = std::io::stdout().is_terminal();
        Self {
            use_color: is_tty && !no_color_flag,
            use_unicode: is_tty && !no_color_flag,
            include_closed: false,
        }
    }
}

/// A single entry in the stack visualization
pub struct StackEntry {
    pub branch: String,
    pub is_current: bool,
    pub is_trunk: bool,
    pub pr: Option<Rc<PullRequest>>,
    pub pr_state: PrState,
    pub timestamp: Option<DateTime<Utc>>,
    pub commits: Vec<CommitInfo>,
    pub extra_commits: usize,
}

/// State of a PR in the stack
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PrState {
    Open,
    Draft,
    Closed,
    Merged,
    NoPr,
}

/// Information about a single commit
#[derive(Clone, Debug)]
pub struct CommitInfo {
    pub sha: String,
    pub message: String,
}

/// Try to open repo from current directory
pub fn detect_repo() -> Option<Repository> {
    Repository::discover(".").ok()
}

/// Try to detect repository (owner/repo) from git remote
///
/// # Arguments
/// * `remote_name` - Name of the remote to use (typically "origin")
pub fn detect_repo_from_remote(remote_name: &str) -> Option<String> {
    let repo = detect_repo()?;
    let remote = repo.find_remote(remote_name).ok()?;
    let url = remote.url()?;
    parse_github_remote_url(url)
}

/// Parse a GitHub remote URL to extract owner/repo
///
/// Handles:
/// - SSH: git@github.com:owner/repo.git
/// - SSH (Enterprise): git@github.mycompany.com:owner/repo.git
/// - HTTPS: https://github.com/owner/repo.git
/// - HTTPS (Enterprise): https://github.mycompany.com/owner/repo.git
/// - Without .git suffix
fn parse_github_remote_url(url: &str) -> Option<String> {
    // SSH format: git@<host>:owner/repo.git
    if url.starts_with("git@") {
        let path = url.split(':').nth(1)?;
        let repo = path.trim_end_matches(".git");
        return Some(repo.to_string());
    }

    // HTTPS format: https://<host>/owner/repo.git
    if url.starts_with("https://") || url.starts_with("http://") {
        let without_protocol = url.split("://").nth(1)?;
        // Skip the host part, get everything after first /
        let path = without_protocol.splitn(2, '/').nth(1)?;
        let repo = path.trim_end_matches(".git");
        return Some(repo.to_string());
    }

    None
}

/// Get current branch name from repo
pub fn current_branch(repo: &Repository) -> Option<String> {
    repo.head().ok()?.shorthand().map(String::from)
}

/// Check if branch exists locally
pub fn branch_exists_locally(repo: &Repository, branch: &str) -> bool {
    repo.find_branch(branch, git2::BranchType::Local).is_ok()
}

/// Get commits between two branches (head..base exclusive)
/// Returns up to MAX_COMMITS and count of extras
pub fn commits_for_branch(repo: &Repository, head: &str, base: &str) -> (Vec<CommitInfo>, usize) {
    let head_commit = match repo.revparse_single(head) {
        Ok(obj) => match obj.peel_to_commit() {
            Ok(c) => c,
            Err(_) => return (vec![], 0),
        },
        Err(_) => return (vec![], 0),
    };

    let base_commit = match repo.revparse_single(base) {
        Ok(obj) => match obj.peel_to_commit() {
            Ok(c) => c,
            Err(_) => return (vec![], 0),
        },
        Err(_) => return (vec![], 0),
    };

    // Find merge base
    let merge_base = match repo.merge_base(head_commit.id(), base_commit.id()) {
        Ok(oid) => oid,
        Err(_) => return (vec![], 0),
    };

    let mut walk = match repo.revwalk() {
        Ok(w) => w,
        Err(_) => return (vec![], 0),
    };

    if walk.set_sorting(Sort::TOPOLOGICAL).is_err() {
        return (vec![], 0);
    }
    if walk.push(head_commit.id()).is_err() {
        return (vec![], 0);
    }
    if walk.hide(merge_base).is_err() {
        return (vec![], 0);
    }

    let mut commits = Vec::new();
    let mut total: usize = 0;

    for oid in walk.flatten() {
        total += 1;
        if commits.len() < MAX_COMMITS {
            if let Ok(commit) = repo.find_commit(oid) {
                let sha = format!("{:.7}", commit.id());
                let message = commit.summary().unwrap_or("").to_string();
                let message = truncate(&message, MAX_MESSAGE_LEN);
                commits.push(CommitInfo { sha, message });
            }
        }
    }

    let extra = total.saturating_sub(MAX_COMMITS);

    (commits, extra)
}

/// Format timestamp as relative time
pub fn format_relative_time(timestamp: &DateTime<Utc>) -> String {
    let now = Utc::now();
    let duration = now.signed_duration_since(*timestamp);

    let seconds = duration.num_seconds();
    if seconds < 0 {
        return "just now".to_string();
    }

    let minutes = duration.num_minutes();
    let hours = duration.num_hours();
    let days = duration.num_days();
    let weeks = days / 7;
    let months = days / 30;
    let years = days / 365;

    if seconds < 60 {
        if seconds == 1 {
            "1 second ago".to_string()
        } else {
            format!("{} seconds ago", seconds)
        }
    } else if minutes < 60 {
        if minutes == 1 {
            "1 minute ago".to_string()
        } else {
            format!("{} minutes ago", minutes)
        }
    } else if hours < 24 {
        if hours == 1 {
            "1 hour ago".to_string()
        } else {
            format!("{} hours ago", hours)
        }
    } else if days < 7 {
        if days == 1 {
            "1 day ago".to_string()
        } else {
            format!("{} days ago", days)
        }
    } else if weeks < 5 {
        if weeks == 1 {
            "1 week ago".to_string()
        } else {
            format!("{} weeks ago", weeks)
        }
    } else if months < 12 {
        if months == 1 {
            "1 month ago".to_string()
        } else {
            format!("{} months ago", months)
        }
    } else if years == 1 {
        "1 year ago".to_string()
    } else {
        format!("{} years ago", years)
    }
}

/// Parse ISO 8601 timestamp from GitHub API
pub fn parse_timestamp(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

/// Truncate string to max length with "..."
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max.saturating_sub(3)).collect();
        format!("{}...", truncated)
    }
}

/// Build stack entries from FlatDep, enriching with local git info if available
/// Filters out closed/merged PRs unless include_closed is true AND branch exists locally
pub fn build_entries(
    stack: &FlatDep,
    repo: Option<&Repository>,
    config: &TreeConfig,
) -> Vec<StackEntry> {
    let current = repo.and_then(current_branch);
    let mut entries = Vec::new();

    // Get the trunk branch from the first PR's base (if stack is not empty)
    let trunk_branch = stack.first().map(|(pr, _)| pr.base().to_string());

    // Process PRs in reverse order (top of stack first)
    for (pr, _parent) in stack.iter().rev() {
        let pr_state = determine_pr_state(pr);

        // Filter closed/merged PRs unless include_closed is set
        if !config.include_closed && (pr_state == PrState::Closed || pr_state == PrState::Merged) {
            continue;
        }

        // If include_closed is set, still filter if branch doesn't exist locally
        if config.include_closed && (pr_state == PrState::Closed || pr_state == PrState::Merged) {
            if let Some(r) = repo {
                if !branch_exists_locally(r, pr.head()) {
                    continue;
                }
            }
        }

        let is_current = current.as_ref().is_some_and(|c| c == pr.head());
        let timestamp = pr.updated_at().and_then(parse_timestamp);

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

        entries.push(StackEntry {
            branch: pr.head().to_string(),
            is_current,
            is_trunk: false,
            pr: Some(pr.clone()),
            pr_state,
            timestamp,
            commits,
            extra_commits,
        });
    }

    // Add trunk branch as final entry
    if let Some(trunk) = trunk_branch {
        let is_current = current.as_ref() == Some(&trunk);

        // Try to get timestamp from trunk branch
        let timestamp = if let Some(r) = repo {
            r.revparse_single(&trunk)
                .ok()
                .and_then(|obj| obj.peel_to_commit().ok())
                .map(|c| {
                    let time = c.time();
                    DateTime::from_timestamp(time.seconds(), 0).unwrap_or_else(Utc::now)
                })
        } else {
            None
        };

        entries.push(StackEntry {
            branch: trunk,
            is_current,
            is_trunk: true,
            pr: None,
            pr_state: PrState::NoPr,
            timestamp,
            commits: vec![],
            extra_commits: 0,
        });
    }

    entries
}

/// Determine PR state from PullRequest
fn determine_pr_state(pr: &PullRequest) -> PrState {
    if pr.is_merged() {
        PrState::Merged
    } else if *pr.state() == PullRequestStatus::Closed {
        PrState::Closed
    } else if pr.is_draft() {
        PrState::Draft
    } else {
        PrState::Open
    }
}

/// Render the visual tree output
pub fn render(entries: &[StackEntry], config: &TreeConfig, has_repo: bool) -> String {
    let mut out = String::new();

    // Symbols based on config
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

        // Branch name + styling for closed/merged
        let branch_display = format_branch(entry, config);

        out.push_str(&format!("{} {}\n", node, branch_display));

        // Connector for content below
        let connector = if is_last { " " } else { pipe };

        // Timestamp line
        if let Some(ts) = &entry.timestamp {
            let time_str = format_relative_time(ts);
            let styled_time = if config.use_color {
                style(&time_str).dim().to_string()
            } else {
                time_str
            };
            out.push_str(&format!("{} {}\n", connector, styled_time));
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

    // Hint if no repo detected
    if !has_repo && !entries.is_empty() {
        out.push_str(
            "\nhint: run from a git repo or use -C <path> to see commits and current branch\n",
        );
    }

    out
}

/// Format branch name with styling for closed/merged PRs
fn format_branch(entry: &StackEntry, config: &TreeConfig) -> String {
    let mut display = entry.branch.clone();

    // Add (current) suffix if this is the current branch
    if entry.is_current {
        display = format!("{} (current)", display);
    }

    // Add draft indicator
    if entry.pr_state == PrState::Draft {
        display = format!("{} (draft)", display);
    }

    if config.use_color {
        match entry.pr_state {
            PrState::Closed | PrState::Merged => style(&display).dim().strikethrough().to_string(),
            _ => display,
        }
    } else {
        match entry.pr_state {
            PrState::Closed => format!("{} [closed]", display),
            PrState::Merged => format!("{} [merged]", display),
            _ => display,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{PullRequest, PullRequestStatus};
    use chrono::Datelike;

    #[test]
    fn test_format_relative_time_seconds() {
        let now = Utc::now();
        let ts = now - chrono::Duration::seconds(8);
        assert_eq!(format_relative_time(&ts), "8 seconds ago");
    }

    #[test]
    fn test_format_relative_time_one_second() {
        let now = Utc::now();
        let ts = now - chrono::Duration::seconds(1);
        assert_eq!(format_relative_time(&ts), "1 second ago");
    }

    #[test]
    fn test_format_relative_time_minutes() {
        let now = Utc::now();
        let ts = now - chrono::Duration::minutes(5);
        assert_eq!(format_relative_time(&ts), "5 minutes ago");
    }

    #[test]
    fn test_format_relative_time_one_minute() {
        let now = Utc::now();
        let ts = now - chrono::Duration::minutes(1);
        assert_eq!(format_relative_time(&ts), "1 minute ago");
    }

    #[test]
    fn test_format_relative_time_hours() {
        let now = Utc::now();
        let ts = now - chrono::Duration::hours(2);
        assert_eq!(format_relative_time(&ts), "2 hours ago");
    }

    #[test]
    fn test_format_relative_time_one_hour() {
        let now = Utc::now();
        let ts = now - chrono::Duration::hours(1);
        assert_eq!(format_relative_time(&ts), "1 hour ago");
    }

    #[test]
    fn test_format_relative_time_days() {
        let now = Utc::now();
        let ts = now - chrono::Duration::days(3);
        assert_eq!(format_relative_time(&ts), "3 days ago");
    }

    #[test]
    fn test_format_relative_time_one_day() {
        let now = Utc::now();
        let ts = now - chrono::Duration::days(1);
        assert_eq!(format_relative_time(&ts), "1 day ago");
    }

    #[test]
    fn test_format_relative_time_weeks() {
        let now = Utc::now();
        let ts = now - chrono::Duration::weeks(3);
        assert_eq!(format_relative_time(&ts), "3 weeks ago");
    }

    #[test]
    fn test_format_relative_time_one_week() {
        let now = Utc::now();
        let ts = now - chrono::Duration::weeks(1);
        assert_eq!(format_relative_time(&ts), "1 week ago");
    }

    #[test]
    fn test_format_relative_time_months() {
        let now = Utc::now();
        let ts = now - chrono::Duration::days(60);
        assert_eq!(format_relative_time(&ts), "2 months ago");
    }

    #[test]
    fn test_format_relative_time_one_year() {
        let now = Utc::now();
        let ts = now - chrono::Duration::days(365);
        assert_eq!(format_relative_time(&ts), "1 year ago");
    }

    #[test]
    fn test_format_relative_time_years() {
        let now = Utc::now();
        let ts = now - chrono::Duration::days(730);
        assert_eq!(format_relative_time(&ts), "2 years ago");
    }

    #[test]
    fn test_truncate_short_string() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_exact_length() {
        assert_eq!(truncate("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_long_string() {
        assert_eq!(truncate("hello world", 8), "hello...");
    }

    #[test]
    fn test_truncate_unicode() {
        assert_eq!(truncate("hello 世界 world", 10), "hello 世...");
    }

    #[test]
    fn test_parse_timestamp_valid() {
        let ts = parse_timestamp("2024-01-15T10:30:00Z");
        assert!(ts.is_some());
        let ts = ts.unwrap();
        assert_eq!(ts.year(), 2024);
        assert_eq!(ts.month(), 1);
        assert_eq!(ts.day(), 15);
    }

    #[test]
    fn test_parse_timestamp_invalid() {
        let ts = parse_timestamp("not a timestamp");
        assert!(ts.is_none());
    }

    #[test]
    fn test_determine_pr_state_open() {
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
        assert_eq!(determine_pr_state(&pr), PrState::Open);
    }

    #[test]
    fn test_determine_pr_state_draft() {
        let pr = PullRequest::new_for_test(
            1,
            "feature",
            "main",
            "Test PR",
            PullRequestStatus::Open,
            true,
            None,
            vec![],
        );
        assert_eq!(determine_pr_state(&pr), PrState::Draft);
    }

    #[test]
    fn test_determine_pr_state_closed() {
        let pr = PullRequest::new_for_test(
            1,
            "feature",
            "main",
            "Test PR",
            PullRequestStatus::Closed,
            false,
            None,
            vec![],
        );
        assert_eq!(determine_pr_state(&pr), PrState::Closed);
    }

    #[test]
    fn test_determine_pr_state_merged() {
        let pr = PullRequest::new_for_test(
            1,
            "feature",
            "main",
            "Test PR",
            PullRequestStatus::Closed,
            false,
            Some("2024-01-15T10:00:00Z".to_string()),
            vec![],
        );
        assert_eq!(determine_pr_state(&pr), PrState::Merged);
    }

    fn make_test_entry(
        branch: &str,
        is_current: bool,
        is_trunk: bool,
        pr_state: PrState,
        timestamp: Option<DateTime<Utc>>,
        commits: Vec<CommitInfo>,
        extra_commits: usize,
    ) -> StackEntry {
        StackEntry {
            branch: branch.to_string(),
            is_current,
            is_trunk,
            pr: None,
            pr_state,
            timestamp,
            commits,
            extra_commits,
        }
    }

    #[test]
    fn test_render_simple_stack_no_color() {
        let config = TreeConfig {
            use_color: false,
            use_unicode: false,
            include_closed: false,
        };

        let entries = vec![
            make_test_entry("feature-2", true, false, PrState::Open, None, vec![], 0),
            make_test_entry("feature-1", false, false, PrState::Open, None, vec![], 0),
            make_test_entry("main", false, true, PrState::NoPr, None, vec![], 0),
        ];

        let output = render(&entries, &config, true);
        assert!(output.contains("* feature-2 (current)"));
        assert!(output.contains("o feature-1"));
        assert!(output.contains("o main"));
    }

    #[test]
    fn test_render_with_closed_pr_no_color() {
        let config = TreeConfig {
            use_color: false,
            use_unicode: false,
            include_closed: true,
        };

        let entries = vec![
            make_test_entry("feature-2", false, false, PrState::Closed, None, vec![], 0),
            make_test_entry("main", false, true, PrState::NoPr, None, vec![], 0),
        ];

        let output = render(&entries, &config, true);
        assert!(output.contains("feature-2 [closed]"));
    }

    #[test]
    fn test_render_with_merged_pr_no_color() {
        let config = TreeConfig {
            use_color: false,
            use_unicode: false,
            include_closed: true,
        };

        let entries = vec![
            make_test_entry("feature-2", false, false, PrState::Merged, None, vec![], 0),
            make_test_entry("main", false, true, PrState::NoPr, None, vec![], 0),
        ];

        let output = render(&entries, &config, true);
        assert!(output.contains("feature-2 [merged]"));
    }

    #[test]
    fn test_render_with_draft_pr() {
        let config = TreeConfig {
            use_color: false,
            use_unicode: false,
            include_closed: false,
        };

        let entries = vec![
            make_test_entry("feature-wip", false, false, PrState::Draft, None, vec![], 0),
            make_test_entry("main", false, true, PrState::NoPr, None, vec![], 0),
        ];

        let output = render(&entries, &config, true);
        assert!(output.contains("feature-wip (draft)"));
    }

    #[test]
    fn test_render_with_commits() {
        let config = TreeConfig {
            use_color: false,
            use_unicode: false,
            include_closed: false,
        };

        let commits = vec![
            CommitInfo {
                sha: "abc1234".to_string(),
                message: "First commit".to_string(),
            },
            CommitInfo {
                sha: "def5678".to_string(),
                message: "Second commit".to_string(),
            },
        ];

        let entries = vec![
            make_test_entry("feature", false, false, PrState::Open, None, commits, 0),
            make_test_entry("main", false, true, PrState::NoPr, None, vec![], 0),
        ];

        let output = render(&entries, &config, true);
        assert!(output.contains("abc1234 - First commit"));
        assert!(output.contains("def5678 - Second commit"));
    }

    #[test]
    fn test_render_with_extra_commits() {
        let config = TreeConfig {
            use_color: false,
            use_unicode: false,
            include_closed: false,
        };

        let commits = vec![CommitInfo {
            sha: "abc1234".to_string(),
            message: "First commit".to_string(),
        }];

        let entries = vec![
            make_test_entry("feature", false, false, PrState::Open, None, commits, 5),
            make_test_entry("main", false, true, PrState::NoPr, None, vec![], 0),
        ];

        let output = render(&entries, &config, true);
        assert!(output.contains("+ 5 more"));
    }

    #[test]
    fn test_render_hint_when_no_repo() {
        let config = TreeConfig {
            use_color: false,
            use_unicode: false,
            include_closed: false,
        };

        let entries = vec![make_test_entry(
            "feature",
            false,
            false,
            PrState::Open,
            None,
            vec![],
            0,
        )];

        let output = render(&entries, &config, false);
        assert!(output.contains("hint: run from a git repo"));
    }

    #[test]
    fn test_render_no_hint_when_repo_present() {
        let config = TreeConfig {
            use_color: false,
            use_unicode: false,
            include_closed: false,
        };

        let entries = vec![make_test_entry(
            "feature",
            false,
            false,
            PrState::Open,
            None,
            vec![],
            0,
        )];

        let output = render(&entries, &config, true);
        assert!(!output.contains("hint:"));
    }

    #[test]
    fn test_render_unicode_symbols() {
        let config = TreeConfig {
            use_color: false,
            use_unicode: true,
            include_closed: false,
        };

        let entries = vec![
            make_test_entry("feature", true, false, PrState::Open, None, vec![], 0),
            make_test_entry("main", false, true, PrState::NoPr, None, vec![], 0),
        ];

        let output = render(&entries, &config, true);
        assert!(output.contains("\u{25C9}")); // ◉
        assert!(output.contains("\u{25EF}")); // ◯
    }

    #[test]
    fn test_build_entries_filters_closed() {
        let pr1 = Rc::new(PullRequest::new_for_test(
            1,
            "feature-1",
            "main",
            "Open PR",
            PullRequestStatus::Open,
            false,
            None,
            vec![],
        ));
        let pr2 = Rc::new(PullRequest::new_for_test(
            2,
            "feature-2",
            "feature-1",
            "Closed PR",
            PullRequestStatus::Closed,
            false,
            None,
            vec![],
        ));

        let stack: FlatDep = vec![(pr1, None), (pr2.clone(), Some(pr2))];

        let config = TreeConfig {
            use_color: false,
            use_unicode: false,
            include_closed: false,
        };

        let entries = build_entries(&stack, None, &config);

        // Should only have open PR + trunk
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].branch, "feature-1");
        assert_eq!(entries[1].branch, "main");
    }

    #[test]
    fn test_format_branch_current() {
        let config = TreeConfig {
            use_color: false,
            use_unicode: false,
            include_closed: false,
        };

        let entry = make_test_entry("feature", true, false, PrState::Open, None, vec![], 0);
        let output = format_branch(&entry, &config);
        assert_eq!(output, "feature (current)");
    }

    #[test]
    fn test_format_branch_draft() {
        let config = TreeConfig {
            use_color: false,
            use_unicode: false,
            include_closed: false,
        };

        let entry = make_test_entry("feature", false, false, PrState::Draft, None, vec![], 0);
        let output = format_branch(&entry, &config);
        assert_eq!(output, "feature (draft)");
    }

    #[test]
    fn test_format_branch_current_and_draft() {
        let config = TreeConfig {
            use_color: false,
            use_unicode: false,
            include_closed: false,
        };

        let entry = make_test_entry("feature", true, false, PrState::Draft, None, vec![], 0);
        let output = format_branch(&entry, &config);
        assert_eq!(output, "feature (current) (draft)");
    }

    // Snapshot tests
    #[test]
    fn test_snapshot_linear_stack_ascii() {
        let config = TreeConfig {
            use_color: false,
            use_unicode: false,
            include_closed: false,
        };

        let entries = vec![
            make_test_entry(
                "pp--06-14-part_3",
                true,
                false,
                PrState::Open,
                None,
                vec![],
                0,
            ),
            make_test_entry(
                "pp--06-14-part_2",
                false,
                false,
                PrState::Open,
                None,
                vec![],
                0,
            ),
            make_test_entry(
                "pp--06-14-part_1",
                false,
                false,
                PrState::Open,
                None,
                vec![],
                0,
            ),
            make_test_entry("main", false, true, PrState::NoPr, None, vec![], 0),
        ];

        let output = render(&entries, &config, true);
        insta::assert_snapshot!(output);
    }

    #[test]
    fn test_snapshot_linear_stack_unicode() {
        let config = TreeConfig {
            use_color: false,
            use_unicode: true,
            include_closed: false,
        };

        let entries = vec![
            make_test_entry(
                "pp--06-14-part_3",
                true,
                false,
                PrState::Open,
                None,
                vec![],
                0,
            ),
            make_test_entry(
                "pp--06-14-part_2",
                false,
                false,
                PrState::Open,
                None,
                vec![],
                0,
            ),
            make_test_entry(
                "pp--06-14-part_1",
                false,
                false,
                PrState::Open,
                None,
                vec![],
                0,
            ),
            make_test_entry("main", false, true, PrState::NoPr, None, vec![], 0),
        ];

        let output = render(&entries, &config, true);
        insta::assert_snapshot!(output);
    }

    #[test]
    fn test_snapshot_stack_with_commits() {
        let config = TreeConfig {
            use_color: false,
            use_unicode: false,
            include_closed: false,
        };

        let commits1 = vec![
            CommitInfo {
                sha: "95338df".to_string(),
                message: "part 3".to_string(),
            },
            CommitInfo {
                sha: "a1b2c3d".to_string(),
                message: "some other commit".to_string(),
            },
        ];

        let commits2 = vec![CommitInfo {
            sha: "95610c6".to_string(),
            message: "part 2".to_string(),
        }];

        let entries = vec![
            make_test_entry(
                "pp--06-14-part_3",
                true,
                false,
                PrState::Open,
                None,
                commits1,
                2,
            ),
            make_test_entry(
                "pp--06-14-part_2",
                false,
                false,
                PrState::Open,
                None,
                commits2,
                0,
            ),
            make_test_entry("main", false, true, PrState::NoPr, None, vec![], 0),
        ];

        let output = render(&entries, &config, true);
        insta::assert_snapshot!(output);
    }

    #[test]
    fn test_snapshot_no_repo_hint() {
        let config = TreeConfig {
            use_color: false,
            use_unicode: false,
            include_closed: false,
        };

        let entries = vec![
            make_test_entry(
                "pp--06-14-part_3",
                false,
                false,
                PrState::Open,
                None,
                vec![],
                0,
            ),
            make_test_entry(
                "pp--06-14-part_2",
                false,
                false,
                PrState::Open,
                None,
                vec![],
                0,
            ),
            make_test_entry("main", false, true, PrState::NoPr, None, vec![], 0),
        ];

        let output = render(&entries, &config, false);
        insta::assert_snapshot!(output);
    }

    #[test]
    fn test_snapshot_with_closed_merged() {
        let config = TreeConfig {
            use_color: false,
            use_unicode: false,
            include_closed: true,
        };

        let entries = vec![
            make_test_entry(
                "pp--06-14-part_3",
                true,
                false,
                PrState::Open,
                None,
                vec![],
                0,
            ),
            make_test_entry(
                "pp--06-14-part_2",
                false,
                false,
                PrState::Merged,
                None,
                vec![],
                0,
            ),
            make_test_entry(
                "pp--06-14-part_1",
                false,
                false,
                PrState::Closed,
                None,
                vec![],
                0,
            ),
            make_test_entry("main", false, true, PrState::NoPr, None, vec![], 0),
        ];

        let output = render(&entries, &config, true);
        insta::assert_snapshot!(output);
    }

    #[test]
    fn test_snapshot_single_pr() {
        let config = TreeConfig {
            use_color: false,
            use_unicode: false,
            include_closed: false,
        };

        let entries = vec![
            make_test_entry(
                "feature-branch",
                false,
                false,
                PrState::Open,
                None,
                vec![],
                0,
            ),
            make_test_entry("main", false, true, PrState::NoPr, None, vec![], 0),
        ];

        let output = render(&entries, &config, true);
        insta::assert_snapshot!(output);
    }

    #[test]
    fn test_snapshot_with_draft() {
        let config = TreeConfig {
            use_color: false,
            use_unicode: false,
            include_closed: false,
        };

        let entries = vec![
            make_test_entry("wip-feature", true, false, PrState::Draft, None, vec![], 0),
            make_test_entry("feature-base", false, false, PrState::Open, None, vec![], 0),
            make_test_entry("main", false, true, PrState::NoPr, None, vec![], 0),
        ];

        let output = render(&entries, &config, true);
        insta::assert_snapshot!(output);
    }

    // Tests for parse_github_remote_url
    #[test]
    fn test_parse_github_remote_url_ssh() {
        assert_eq!(
            parse_github_remote_url("git@github.com:owner/repo.git"),
            Some("owner/repo".to_string())
        );
    }

    #[test]
    fn test_parse_github_remote_url_ssh_no_suffix() {
        assert_eq!(
            parse_github_remote_url("git@github.com:owner/repo"),
            Some("owner/repo".to_string())
        );
    }

    #[test]
    fn test_parse_github_remote_url_https() {
        assert_eq!(
            parse_github_remote_url("https://github.com/owner/repo.git"),
            Some("owner/repo".to_string())
        );
    }

    #[test]
    fn test_parse_github_remote_url_https_no_suffix() {
        assert_eq!(
            parse_github_remote_url("https://github.com/owner/repo"),
            Some("owner/repo".to_string())
        );
    }

    #[test]
    fn test_parse_github_remote_url_http() {
        assert_eq!(
            parse_github_remote_url("http://github.com/owner/repo.git"),
            Some("owner/repo".to_string())
        );
    }

    #[test]
    fn test_parse_github_remote_url_enterprise_ssh() {
        assert_eq!(
            parse_github_remote_url("git@github.mycompany.com:org/project.git"),
            Some("org/project".to_string())
        );
    }

    #[test]
    fn test_parse_github_remote_url_enterprise_https() {
        assert_eq!(
            parse_github_remote_url("https://github.mycompany.com/org/project.git"),
            Some("org/project".to_string())
        );
    }

    #[test]
    fn test_parse_github_remote_url_invalid() {
        assert_eq!(parse_github_remote_url("not-a-url"), None);
    }

    #[test]
    fn test_parse_github_remote_url_empty() {
        assert_eq!(parse_github_remote_url(""), None);
    }
}
