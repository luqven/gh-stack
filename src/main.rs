use clap::{App, AppSettings, Arg, ArgMatches, SubCommand};
use console::style;
use git2::Repository;
use regex::Regex;
use std::env;
use std::error::Error;
use std::rc::Rc;

use gh_stack::api::PullRequest;
use gh_stack::graph::FlatDep;
use gh_stack::land::{self, LandError, LandOptions};
use gh_stack::util::loop_until_confirm;
use gh_stack::Credentials;
use gh_stack::{api, git, graph, markdown, persist, tree};

fn clap<'a, 'b>() -> App<'a, 'b> {
    let identifier = Arg::with_name("identifier")
        .index(1)
        .required(true)
        .help("All pull requests containing this identifier in their title form a stack");

    let repository = Arg::with_name("repository")
        .long("repository")
        .short("r")
        .takes_value(true)
        .help("Remote repository to filter identifier search results by");

    let exclude = Arg::with_name("exclude")
        .long("excl")
        .short("e")
        .multiple(true)
        .takes_value(true)
        .help("Exclude an issue from consideration (by number). Pass multiple times");

    let ci = Arg::with_name("ci")
        .long("ci")
        .takes_value(false)
        .help("Skip waiting for confirmation");

    let prefix = Arg::with_name("prefix")
        .long("prefix")
        .takes_value(true)
        .help("PR title prefix identifier to remove from the title");

    let badges = Arg::with_name("badges")
        .long("badges")
        .takes_value(false)
        .help("Use shields.io badges for PR status (requires public repo visibility)");

    let origin = Arg::with_name("origin")
        .long("origin")
        .short("o")
        .takes_value(true)
        .default_value("origin")
        .help("Name of the git remote to detect repository from (default: origin)");

    let annotate = SubCommand::with_name("annotate")
        .about("Annotate the descriptions of all PRs in a stack with metadata about all PRs in the stack")
        .setting(AppSettings::ArgRequiredElseHelp)
        .arg(identifier.clone())
        .arg(exclude.clone())
        .arg(repository.clone())
        .arg(origin.clone())
        .arg(ci.clone())
        .arg(prefix.clone())
        .arg(badges.clone())
        .arg(Arg::with_name("prelude")
                .long("prelude")
                .short("p")
                .value_name("FILE")
                .help("Prepend the annotation with the contents of this file"));

    let log = SubCommand::with_name("log")
        .about("Print a visual tree of all pull requests in a stack")
        .setting(AppSettings::ArgRequiredElseHelp)
        .arg(identifier.clone())
        .arg(exclude.clone())
        .arg(repository.clone())
        .arg(origin.clone())
        .arg(
            Arg::with_name("short")
                .long("short")
                .short("s")
                .takes_value(false)
                .help("Use compact list format instead of tree view"),
        )
        .arg(
            Arg::with_name("project")
                .long("project")
                .short("C")
                .value_name("PATH")
                .help("Path to local repository (auto-detected if omitted)"),
        )
        .arg(
            Arg::with_name("include-closed")
                .long("include-closed")
                .help("Show local branches whose remote PRs are closed or merged"),
        )
        .arg(
            Arg::with_name("no-color")
                .long("no-color")
                .help("Disable colors and Unicode characters"),
        );

    let autorebase = SubCommand::with_name("autorebase")
        .about("Rebuild a stack based on changes to local branches and mirror these changes up to the remote")
        .arg(Arg::with_name("origin")
                .long("origin")
                .short("o")
                .value_name("ORIGIN")
                .help("Name of the origin to (force-)push the updated stack to (default: `origin`)"))
        .arg(Arg::with_name("project")
                .long("project")
                .short("C")
                .value_name("PATH_TO_PROJECT")
                .help("Path to a local copy of the repository"))
        .arg(Arg::with_name("boundary")
                .long("initial-cherry-pick-boundary")
                .short("b")
                .value_name("SHA")
                .help("Stop the initial cherry-pick at this SHA (exclusive)"))
        .setting(AppSettings::ArgRequiredElseHelp)
        .arg(exclude.clone())
        .arg(repository.clone())
        .arg(ci.clone())
        .arg(identifier.clone());

    let rebase = SubCommand::with_name("rebase")
        .about(
            "Print a bash script to STDOUT that can rebase/update the stack (with a little help)",
        )
        .setting(AppSettings::ArgRequiredElseHelp)
        .arg(exclude.clone())
        .arg(identifier.clone());

    let land = SubCommand::with_name("land")
        .about("Land a stack of PRs by merging the topmost mergeable PR and closing the rest")
        .setting(AppSettings::ArgRequiredElseHelp)
        .arg(identifier.clone())
        .arg(exclude.clone())
        .arg(repository.clone())
        .arg(origin.clone())
        .arg(
            Arg::with_name("no-approval")
                .long("no-approval")
                .takes_value(false)
                .help("Skip approval requirement check"),
        )
        .arg(
            Arg::with_name("count")
                .long("count")
                .takes_value(true)
                .value_name("N")
                .help("Only land the bottom N PRs in the stack"),
        )
        .arg(
            Arg::with_name("dry-run")
                .long("dry-run")
                .takes_value(false)
                .help("Preview what would happen without making changes"),
        );

    let app = App::new("gh-stack")
        .setting(AppSettings::SubcommandRequiredElseHelp)
        .setting(AppSettings::DisableVersion)
        .setting(AppSettings::VersionlessSubcommands)
        .setting(AppSettings::DisableHelpSubcommand)
        .subcommand(annotate)
        .subcommand(log)
        .subcommand(rebase)
        .subcommand(autorebase)
        .subcommand(land);

    app
}

async fn build_pr_stack(
    pattern: &str,
    credentials: &Credentials,
    exclude: Vec<String>,
) -> Result<FlatDep, Box<dyn Error>> {
    let prs = api::search::fetch_pull_requests_matching(pattern, credentials).await?;

    let prs = prs
        .into_iter()
        .filter(|pr| !exclude.contains(&pr.number().to_string()))
        .map(Rc::new)
        .collect::<Vec<Rc<PullRequest>>>();
    let graph = graph::build(&prs);
    let stack = graph::log(&graph);
    Ok(stack)
}

async fn build_pr_stack_for_repo(
    pattern: &str,
    repository: &str,
    credentials: &Credentials,
    exclude: Vec<String>,
) -> Result<FlatDep, Box<dyn Error>> {
    let prs =
        api::search::fetch_matching_pull_requests_from_repository(pattern, repository, credentials)
            .await?;

    let prs = prs
        .into_iter()
        .filter(|pr| !exclude.contains(&pr.number().to_string()))
        .map(Rc::new)
        .collect::<Vec<Rc<PullRequest>>>();
    let graph = graph::build(&prs);
    let stack = graph::log(&graph);
    Ok(stack)
}

fn get_excluded(m: &ArgMatches) -> Vec<String> {
    let excluded = m.values_of("exclude");

    match excluded {
        Some(excluded) => excluded.map(String::from).collect(),
        None => vec![],
    }
}

/// Resolve the repository to use, with fallback chain:
/// 1. -r flag (explicit override)
/// 2. GHSTACK_TARGET_REPOSITORY env var
/// 3. Auto-detect from git remote
fn resolve_repository(
    arg_value: Option<&str>,
    env_value: &str,
    remote_name: &str,
) -> Result<String, String> {
    // Priority 1: Explicit -r flag
    if let Some(repo) = arg_value {
        if !repo.is_empty() {
            return Ok(repo.to_string());
        }
    }

    // Priority 2: Environment variable
    if !env_value.is_empty() {
        return Ok(env_value.to_string());
    }

    // Priority 3: Auto-detect from git remote
    if let Some(repo) = tree::detect_repo_from_remote(remote_name) {
        eprintln!(
            "Detected repository: {} (from {} remote)",
            style(&repo).cyan(),
            remote_name
        );
        return Ok(repo);
    }

    Err("Could not determine repository. Either:\n  \
         - Run from inside a git repo with a GitHub remote\n  \
         - Set GHSTACK_TARGET_REPOSITORY environment variable\n  \
         - Use the -r flag"
        .to_string())
}

fn remove_title_prefixes(title: String, prefix: &str) -> String {
    let regex = Regex::new(&format!("[{}]", prefix).to_string()).unwrap();
    regex.replace_all(&title, "").into_owned()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::from_filename(".gh-stack.env").ok();

    let token = env::var("GHSTACK_OAUTH_TOKEN").expect("You didn't pass `GHSTACK_OAUTH_TOKEN`");
    // store the value of GHSTACK_TARGET_REPOSITORY
    let repository = env::var("GHSTACK_TARGET_REPOSITORY").unwrap_or_default();
    let credentials = Credentials::new(&token);
    let matches = clap().get_matches();

    match matches.subcommand() {
        ("annotate", Some(m)) => {
            let identifier = m.value_of("identifier").unwrap();
            let prefix = m.value_of("prefix").unwrap_or("[]");
            let prefix = regex::escape(prefix);
            // if ci flag is set, set ci to true
            let ci = m.is_present("ci");
            // resolve repository with fallback chain
            let remote_name = m.value_of("origin").unwrap_or("origin");
            let repository = resolve_repository(m.value_of("repository"), &repository, remote_name)
                .unwrap_or_else(|e| panic!("{}", e));

            let identifier = remove_title_prefixes(identifier.to_string(), &prefix);

            println!(
                "Searching for {} identifier in {} repo",
                style(&identifier).bold(),
                style(&repository).bold()
            );

            let stack =
                build_pr_stack_for_repo(&identifier, &repository, &credentials, get_excluded(m))
                    .await?;

            let use_badges = m.is_present("badges");
            let table = markdown::build_table(
                &stack,
                &identifier,
                m.value_of("prelude"),
                &repository,
                use_badges,
            );

            for (pr, _) in stack.iter() {
                println!("{}: {}", pr.number(), pr.title());
            }
            if ci {
                println!("\nCI flag present, skipping confirmation...");
            } else {
                loop_until_confirm("Going to update these PRs ☝️ ");
            }

            persist::persist(&stack, &table, &credentials, &prefix).await?;

            println!("Done!");
        }

        ("log", Some(m)) => {
            let identifier = m.value_of("identifier").unwrap();

            // resolve repository with fallback chain
            let remote_name = m.value_of("origin").unwrap_or("origin");
            let repository = resolve_repository(m.value_of("repository"), &repository, remote_name)
                .unwrap_or_else(|e| panic!("{}", e));

            println!(
                "Searching for {} identifier in {} repo",
                style(identifier).bold(),
                style(&repository).bold()
            );
            let stack =
                build_pr_stack_for_repo(identifier, &repository, &credentials, get_excluded(m))
                    .await?;

            // Check for empty stack
            if stack.is_empty() {
                println!("No PRs found matching '{}'", identifier);
                return Ok(());
            }

            // Check if "short" mode
            if m.is_present("short") {
                // Original flat output
                for (pr, maybe_parent) in stack {
                    match maybe_parent {
                        Some(parent) => {
                            let into = style(format!("(Merges into #{})", parent.number())).green();
                            println!("#{}: {} {}", pr.number(), pr.title(), into);
                        }

                        None => {
                            let into = style("(Base)").red();
                            println!("#{}: {} {}", pr.number(), pr.title(), into);
                        }
                    }
                }
            } else {
                // New tree view (default)
                let no_color = m.is_present("no-color");
                let mut config = tree::TreeConfig::detect(no_color);
                config.include_closed = m.is_present("include-closed");

                let repo = m
                    .value_of("project")
                    .and_then(|p| Repository::open(p).ok())
                    .or_else(tree::detect_repo);

                let entries = tree::build_entries(&stack, repo.as_ref(), &config);
                let output = tree::render(&entries, &config, repo.is_some());
                print!("{}", output);
            }
        }

        ("rebase", Some(m)) => {
            let identifier = m.value_of("identifier").unwrap();
            let stack = build_pr_stack(identifier, &credentials, get_excluded(m)).await?;

            let script = git::generate_rebase_script(stack);
            println!("{}", script);
        }

        ("autorebase", Some(m)) => {
            let identifier = m.value_of("identifier").unwrap();

            // defaults to "origin" if no remote is specified
            let remote_name = m.value_of("origin").unwrap_or("origin");

            // resolve repository with fallback chain
            let repository = resolve_repository(m.value_of("repository"), &repository, remote_name)
                .unwrap_or_else(|e| panic!("{}", e));

            println!(
                "Searching for {} identifier in {} repo",
                style(identifier).bold(),
                style(&repository).bold()
            );
            let stack =
                build_pr_stack_for_repo(identifier, &repository, &credentials, get_excluded(m))
                    .await?;

            let project = m
                .value_of("project")
                .expect("The --project argument is required.");
            let project = Repository::open(project)?;

            // use the same remote name for finding the remote to push to
            let remote = project.find_remote(remote_name).unwrap();

            // if ci flag is set, set ci to true
            let ci = m.is_present("ci");

            git::perform_rebase(
                stack,
                &project,
                remote.name().unwrap(),
                m.value_of("boundary"),
                ci,
            )
            .await?;
            println!("All done!");
        }

        ("land", Some(m)) => {
            let identifier = m.value_of("identifier").unwrap();

            // resolve repository with fallback chain
            let remote_name = m.value_of("origin").unwrap_or("origin");
            let repository = resolve_repository(m.value_of("repository"), &repository, remote_name)
                .unwrap_or_else(|e| panic!("{}", e));

            println!(
                "Analyzing stack for {} in {}...\n",
                style(identifier).bold(),
                style(&repository).bold()
            );

            let stack =
                build_pr_stack_for_repo(identifier, &repository, &credentials, get_excluded(m))
                    .await?;

            if stack.is_empty() {
                println!("No PRs found matching '{}'", identifier);
                return Ok(());
            }

            // Parse options
            let require_approval = !m.is_present("no-approval");
            let max_count = m
                .value_of("count")
                .map(|s| s.parse::<usize>().expect("--count must be a number"));
            let dry_run = m.is_present("dry-run");

            let options = LandOptions {
                require_approval,
                max_count,
            };

            // Create the landing plan
            let plan = match land::create_land_plan(&stack, &repository, &options) {
                Ok(plan) => plan,
                Err(e) => {
                    match &e {
                        LandError::ApprovalRequired { pr_number } => {
                            eprintln!(
                                "{} PR #{} requires approval",
                                style("Error:").red().bold(),
                                pr_number
                            );
                            eprintln!(
                                "  Hint: Get approval for #{}, or use {} to skip this check",
                                pr_number,
                                style("--no-approval").cyan()
                            );
                        }
                        LandError::DraftBlocking { pr_number } => {
                            eprintln!(
                                "{} PR #{} is a draft and blocks landing",
                                style("Error:").red().bold(),
                                pr_number
                            );
                            eprintln!(
                                "  Hint: Mark PR #{} as ready for review before landing",
                                pr_number
                            );
                        }
                        _ => {
                            eprintln!("{} {}", style("Error:").red().bold(), e);
                        }
                    }
                    std::process::exit(1);
                }
            };

            // Calculate remaining PRs (those not in the plan)
            let plan_pr_numbers: Vec<usize> = std::iter::once(plan.top_pr.number())
                .chain(plan.prs_to_close.iter().map(|pr| pr.number()))
                .collect();
            let remaining_prs: Vec<Rc<PullRequest>> = stack
                .iter()
                .filter(|(pr, _)| !plan_pr_numbers.contains(&pr.number()))
                .filter(|(pr, _)| !pr.is_merged() && pr.state() == &api::PullRequestStatus::Open)
                .map(|(pr, _)| pr.clone())
                .collect();

            if dry_run {
                // Print dry-run output
                println!("{}", land::format_dry_run(&plan, &remaining_prs));
                return Ok(());
            }

            // Execute the landing
            let total_to_land = plan.prs_to_close.len() + 1;
            println!("Landing {} PR(s)...\n", total_to_land);

            match land::execute_land(&plan, &credentials).await {
                Ok(result) => {
                    println!(
                        "\n{} Stack landed via {}",
                        style("Done!").green().bold(),
                        style(&result.merge_url).cyan()
                    );
                }
                Err(e) => {
                    eprintln!("\n{} {}", style("Error:").red().bold(), e);
                    std::process::exit(1);
                }
            }
        }

        (_, _) => panic!("Invalid subcommand."),
    }

    Ok(())
    /*
    # TODO
    - [x] Authentication (personal access token)
    - [x] Fetch all PRs matching Jira
    - [x] Construct graph
    - [x] Create markdown table
    - [x] Persist table back to Github
    - [x] Accept a prelude via STDIN
    - [x] Log a textual representation of the graph
    - [x] Automate rebase
    - [x] Better CLI args
    - [x] PR status icons
    - [ ] Build status icons
    - [ ] Panic on non-200s
    */
}
