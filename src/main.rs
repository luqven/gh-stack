use clap::{App, AppSettings, Arg, ArgMatches, SubCommand};
use console::style;
use git2::Repository;
use regex::Regex;
use std::env;
use std::error::Error;
use std::rc::Rc;

use gh_stack::api::PullRequest;
use gh_stack::graph::FlatDep;
use gh_stack::util::loop_until_confirm;
use gh_stack::Credentials;
use gh_stack::{api, git, graph, markdown, persist};

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

    let annotate = SubCommand::with_name("annotate")
        .about("Annotate the descriptions of all PRs in a stack with metadata about all PRs in the stack")
        .setting(AppSettings::ArgRequiredElseHelp)
        .arg(identifier.clone())
        .arg(exclude.clone())
        .arg(repository.clone())
        .arg(ci.clone())
        .arg(prefix.clone())
        .arg(Arg::with_name("prelude")
                .long("prelude")
                .short("p")
                .value_name("FILE")
                .help("Prepend the annotation with the contents of this file"));

    let log = SubCommand::with_name("log")
        .about("Print a list of all pull requests in a stack to STDOUT")
        .setting(AppSettings::ArgRequiredElseHelp)
        .arg(exclude.clone())
        .arg(identifier.clone())
        .arg(repository.clone());

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

    let app = App::new("gh-stack")
        .setting(AppSettings::SubcommandRequiredElseHelp)
        .setting(AppSettings::DisableVersion)
        .setting(AppSettings::VersionlessSubcommands)
        .setting(AppSettings::DisableHelpSubcommand)
        .subcommand(annotate)
        .subcommand(log)
        .subcommand(rebase)
        .subcommand(autorebase);

    app
}

async fn build_pr_stack(
    pattern: &str,
    credentials: &Credentials,
    exclude: Vec<String>,
) -> Result<FlatDep, Box<dyn Error>> {
    let prs = api::search::fetch_pull_requests_matching(pattern, &credentials).await?;

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
    let prs = api::search::fetch_matching_pull_requests_from_repository(
        pattern,
        repository,
        &credentials,
    )
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

fn remove_title_prefixes(title: String, prefix: &str) -> String {
    let regex = Regex::new(&format!("[{}]", prefix).to_string()).unwrap();
    let result = regex.replace_all(&title, "").into_owned();
    return result;
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
            // replace it with the -r argument value if set
            let repository = m.value_of("repository").unwrap_or(&repository);
            // if repository is still unset, throw an error
            if repository.is_empty() {
                let error = format!(
                    "Invalid target repository {repo}. You must pass a repository with the -r flag or set GHSTACK_TARGET_REPOSITORY", repo = repository
                );
                panic!("{}", error);
            }

            let identifier = remove_title_prefixes(identifier.to_string(), &prefix);

            println!(
                "Searching for {} identifier in {} repo",
                style(&identifier).bold(),
                style(repository).bold()
            );

            let stack =
                build_pr_stack_for_repo(&identifier, repository, &credentials, get_excluded(m))
                    .await?;

            let table =
                markdown::build_table(&stack, &identifier, m.value_of("prelude"), repository);

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
            // replace it with the -r argument value if set
            let repository = m.value_of("repository").unwrap_or(&repository);
            // if repository is still unset, throw an error
            if repository.is_empty() {
                panic!(
                    "You must pass a repository with the -r flag or set GHSTACK_TARGET_REPOSITORY"
                );
            }

            println!(
                "Searching for {} identifier in {} repo",
                style(identifier).bold(),
                style(repository).bold()
            );
            let stack =
                build_pr_stack_for_repo(identifier, repository, &credentials, get_excluded(m))
                    .await?;

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
        }

        ("rebase", Some(m)) => {
            let identifier = m.value_of("identifier").unwrap();
            let stack = build_pr_stack(identifier, &credentials, get_excluded(m)).await?;

            let script = git::generate_rebase_script(stack);
            println!("{}", script);
        }

        ("autorebase", Some(m)) => {
            let identifier = m.value_of("identifier").unwrap();

            // store the value of GHSTACK_TARGET_REPOSITORY
            let repository = env::var("GHSTACK_TARGET_REPOSITORY").unwrap_or_default();
            // replace it with the -r argument value if set
            let repository = m.value_of("repository").unwrap_or(&repository);
            // if repository is still unset, throw an error
            if repository.is_empty() {
                panic!(
                    "You must pass a repository with the -r flag or set GHSTACK_TARGET_REPOSITORY"
                );
            }

            println!(
                "Searching for {} identifier in {} repo",
                style(identifier).bold(),
                style(repository).bold()
            );
            let stack =
                build_pr_stack_for_repo(identifier, repository, &credentials, get_excluded(m))
                    .await?;

            let project = m
                .value_of("project")
                .expect("The --project argument is required.");
            let project = Repository::open(project)?;

            // defaults to "origin" if no remote is specified
            let remote = m.value_of("origin").unwrap_or("origin");
            let remote = project.find_remote(remote).unwrap();

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
