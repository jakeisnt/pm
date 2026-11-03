//! Minimal GitHub REST client: just the endpoints `p` uses.

use anyhow::{Context, Result, bail};
use reqwest::{Method, RequestBuilder, StatusCode};
use serde::{Deserialize, de::DeserializeOwned};

const API: &str = "https://api.github.com";

#[derive(Debug, Deserialize)]
pub struct Repo {
    pub full_name: String,
    pub clone_url: String,
}

#[derive(Debug, Deserialize)]
struct User {
    login: String,
}

pub struct GitHub {
    http: reqwest::Client,
    token: Option<String>,
}

impl GitHub {
    pub fn new(token: Option<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            token,
        }
    }

    pub fn token(&self) -> Option<&str> {
        self.token.as_deref()
    }

    fn request(&self, method: Method, path: &str) -> RequestBuilder {
        let request = self
            .http
            .request(method, format!("{API}{path}"))
            .header(reqwest::header::USER_AGENT, "pm")
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28");
        match &self.token {
            Some(token) => request.bearer_auth(token),
            None => request,
        }
    }

    async fn send<T: DeserializeOwned>(request: RequestBuilder, what: &str) -> Result<T> {
        let response = request
            .send()
            .await
            .with_context(|| format!("failed to {what}"))?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            bail!("failed to {what}: {status} {body}");
        }
        response
            .json()
            .await
            .with_context(|| format!("failed to parse response to {what}"))
    }

    /// The authenticated user's login, or `None` when the token is missing or rejected.
    pub async fn current_user(&self) -> Result<Option<String>> {
        if self.token.is_none() {
            return Ok(None);
        }
        let response = self.request(Method::GET, "/user").send().await?;
        if response.status() == StatusCode::UNAUTHORIZED {
            return Ok(None);
        }
        Ok(Some(
            response.error_for_status()?.json::<User>().await?.login,
        ))
    }

    pub async fn repo(&self, full_name: &str) -> Result<Repo> {
        Self::send(
            self.request(Method::GET, &format!("/repos/{full_name}")),
            &format!("look up GitHub repository {full_name}"),
        )
        .await
    }

    /// Create a private repository, under `org` when given, else the current user.
    pub async fn create_repo(
        &self,
        org: Option<&str>,
        name: &str,
        description: Option<&str>,
    ) -> Result<Repo> {
        let path = match org {
            Some(org) => format!("/orgs/{org}/repos"),
            None => "/user/repos".to_string(),
        };
        let body = serde_json::json!({
            "name": name,
            "private": true,
            "description": description,
        });
        Self::send(
            self.request(Method::POST, &path).json(&body),
            &format!("create GitHub repository {name}"),
        )
        .await
    }

    pub async fn rename_repo(&self, full_name: &str, new_name: &str) -> Result<Repo> {
        Self::send(
            self.request(Method::PATCH, &format!("/repos/{full_name}"))
                .json(&serde_json::json!({ "name": new_name })),
            &format!("rename GitHub repository {full_name}"),
        )
        .await
    }

    /// Every repository the user owns, collaborates on, or can see through an org.
    pub async fn user_repos(&self) -> Result<Vec<Repo>> {
        let mut repos = Vec::new();
        for page in 1.. {
            let batch: Vec<Repo> = Self::send(
                self.request(Method::GET, "/user/repos").query(&[
                    ("affiliation", "owner,collaborator,organization_member"),
                    ("per_page", "100"),
                    ("page", &page.to_string()),
                ]),
                "list GitHub repositories",
            )
            .await?;
            let done = batch.len() < 100;
            repos.extend(batch);
            if done {
                break;
            }
        }
        Ok(repos)
    }
}
