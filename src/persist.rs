use futures::future::join_all;
use regex::Regex;
use std::error::Error;

use crate::api::pull_request;
use crate::graph::FlatDep;
use crate::Credentials;

const SHIELD_OPEN: &str = "<!---GHSTACKOPEN-->";
const SHIELD_CLOSE: &str = "<!---GHSTACKCLOSE-->";

fn safe_replace(body: &str, table: &str) -> String {
    let new = format!("\n{}\n{}\n{}\n", SHIELD_OPEN, table, SHIELD_CLOSE);

    if body.contains(SHIELD_OPEN) {
        let matcher = format!(
            "(?s){}.*{}",
            regex::escape(SHIELD_OPEN),
            regex::escape(SHIELD_CLOSE)
        );
        let re = Regex::new(&matcher).unwrap();
        re.replace_all(body, &new[..]).into_owned()
    } else {
        let mut body: String = body.to_owned();
        body.push_str(&new);
        body
    }
}

/**
 * Remove title prefixes from markdown table
 */
fn remove_title_prefixes(row: String, prefix: &str) -> String {
    let prefix = String::from(prefix);
    let prefix_1 = &prefix[0..2];
    let prefix_2 = &prefix[2..4];
    // Regex removes the prefix from the title and removes surrounding whitespace
    let regex_str = format!(r"\s*{}[^\]]+{}\s*", prefix_1, prefix_2);
    let regex = Regex::new(&regex_str).unwrap();
    return regex.replace_all(&row, "").trim().to_string().to_owned();
}

pub async fn persist(
    prs: &FlatDep,
    table: &str,
    c: &Credentials,
    prefix: &str,
) -> Result<(), Box<dyn Error>> {
    let futures = prs.iter().map(|(pr, _)| {
        let body = table.replace(&pr.title()[..], &format!("👉 {}", pr.title())[..]);
        let body = remove_title_prefixes(body, prefix);
        let description = safe_replace(pr.body(), body.as_ref());
        pull_request::update_description(description, pr.clone(), c)
    });

    let results = join_all(futures.collect::<Vec<_>>()).await;

    for result in results {
        result.unwrap();
    }

    Ok(())
}
