use serde::Serialize;
use std::fmt;

#[derive(Debug, Clone, Serialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
    pub source: String,
    pub scope: Option<String>,
    pub github_full_name: Option<String>,
}

impl fmt::Display for Project {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:<28} {}", self.name, self.path)
    }
}
