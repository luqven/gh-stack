pub mod api;
pub mod git;
pub mod graph;
pub mod identifier;
pub mod land;
pub mod markdown;
pub mod persist;
pub mod status;
pub mod tree;
pub mod util;

pub struct Credentials {
    // Personal access token
    pub(crate) token: String,
}

impl Credentials {
    pub fn new(token: &str) -> Credentials {
        Credentials {
            token: token.to_string(),
        }
    }
}
