//! Minimal GitHub REST client: just the endpoints `p` uses.
//!
//! GitHub is treated as unreliable: every request has a timeout, reads are
//! retried with backoff on transient failures (network errors, timeouts, 5xx,
//! rate limits), and failures carry a [`Failure`] so callers can tell "this
//! repo doesn't exist" apart from "GitHub is having a bad day".

use anyhow::{Context, Result};
use reqwest::{Method, RequestBuilder, Response, StatusCode};
use serde::{Deserialize, de::DeserializeOwned};
use std::{fmt, time::Duration};

const DEFAULT_API: &str = "https://api.github.com";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_BACKOFF: Duration = Duration::from_secs(8);

#[derive(Debug, Deserialize)]
pub struct Repo {
    pub full_name: String,
    pub clone_url: String,
}

#[derive(Debug, Deserialize)]
struct User {
    login: String,
}

/// Why a GitHub call failed; recover it with `err.downcast_ref::<Failure>()`.
#[derive(Debug)]
pub enum Failure {
    /// 404: the thing doesn't exist (or the token can't see it).
    NotFound,
    /// A definite answer we shouldn't retry (401, 403 without rate limit, 422, ...).
    Rejected(StatusCode, String),
    /// Network trouble, timeouts, 5xx or rate limiting, after retries ran out.
    Unavailable(String),
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Failure::NotFound => write!(f, "not found on GitHub"),
            Failure::Rejected(status, body) => {
                write!(f, "GitHub rejected the request: {status} {body}")
            }
            Failure::Unavailable(why) => write!(f, "GitHub is unavailable: {why}"),
        }
    }
}

impl std::error::Error for Failure {}

/// True when `err` means GitHub (or the network) was down, not a real "no".
pub fn is_unavailable(err: &anyhow::Error) -> bool {
    matches!(err.downcast_ref::<Failure>(), Some(Failure::Unavailable(_)))
}

pub fn is_not_found(err: &anyhow::Error) -> bool {
    matches!(err.downcast_ref::<Failure>(), Some(Failure::NotFound))
}

pub struct GitHub {
    http: reqwest::Client,
    api: String,
    token: Option<String>,
    attempts: u32,
}

impl GitHub {
    /// A client for interactive use: two attempts, so a flaky GitHub costs
    /// seconds rather than minutes.
    pub fn new(token: Option<String>) -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .unwrap_or_default();
        Self {
            http,
            // Overridable for GitHub Enterprise (and tests).
            api: std::env::var("PM_GITHUB_API_URL").unwrap_or_else(|_| DEFAULT_API.to_string()),
            token,
            attempts: 2,
        }
    }

    /// More patient retries, for the background sync where nobody is waiting.
    pub fn patient(mut self) -> Self {
        self.attempts = 5;
        self
    }

    pub fn token(&self) -> Option<&str> {
        self.token.as_deref()
    }

    fn request(&self, method: Method, path: &str) -> RequestBuilder {
        let request = self
            .http
            .request(method, format!("{}{path}", self.api))
            .header(reqwest::header::USER_AGENT, "pm")
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28");
        match &self.token {
            Some(token) => request.bearer_auth(token),
            None => request,
        }
    }

    /// Send a request, retrying transient failures when it is safe to.
    /// Writes (`retry = false`) are sent once: repeating a create could act twice.
    async fn send(&self, build: impl Fn() -> RequestBuilder, retry: bool) -> Result<Response> {
        let attempts = if retry { self.attempts } else { 1 };
        let mut last = String::new();
        for attempt in 1..=attempts {
            let wait = match build().send().await {
                Ok(response) => match classify(response).await {
                    Ok(response) => return Ok(response),
                    Err((Failure::Unavailable(why), wait)) => {
                        last = why;
                        wait
                    }
                    Err((failure, _)) => return Err(failure.into()),
                },
                Err(e) => {
                    last = if e.is_timeout() {
                        "request timed out".into()
                    } else {
                        format!("network error: {e}")
                    };
                    None
                }
            };
            if attempt < attempts {
                let backoff = Duration::from_millis(500 * 2u64.pow(attempt - 1));
                tokio::time::sleep(wait.unwrap_or(backoff).min(MAX_BACKOFF)).await;
            }
        }
        Err(Failure::Unavailable(last).into())
    }

    async fn get<T: DeserializeOwned>(&self, path: &str, query: &[(&str, &str)]) -> Result<T> {
        let response = self
            .send(|| self.request(Method::GET, path).query(query), true)
            .await?;
        response
            .json()
            .await
            .with_context(|| format!("unexpected response from GitHub for {path}"))
    }

    async fn write<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<T> {
        let response = self
            .send(|| self.request(method.clone(), path).json(body), false)
            .await?;
        response
            .json()
            .await
            .with_context(|| format!("unexpected response from GitHub for {path}"))
    }

    /// The authenticated user's login, or `None` when the token is missing or rejected.
    pub async fn current_user(&self) -> Result<Option<String>> {
        if self.token.is_none() {
            return Ok(None);
        }
        match self.get::<User>("/user", &[]).await {
            Ok(user) => Ok(Some(user.login)),
            Err(e)
                if matches!(
                    e.downcast_ref(),
                    Some(Failure::Rejected(StatusCode::UNAUTHORIZED, _))
                ) =>
            {
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }

    pub async fn repo(&self, full_name: &str) -> Result<Repo> {
        self.get(&format!("/repos/{full_name}"), &[])
            .await
            .with_context(|| format!("looking up {full_name}"))
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
        self.write(Method::POST, &path, &body)
            .await
            .with_context(|| format!("creating GitHub repository {name}"))
    }

    pub async fn rename_repo(&self, full_name: &str, new_name: &str) -> Result<Repo> {
        self.write(
            Method::PATCH,
            &format!("/repos/{full_name}"),
            &serde_json::json!({ "name": new_name }),
        )
        .await
        .with_context(|| format!("renaming GitHub repository {full_name}"))
    }

    /// Every repository the user owns, collaborates on, or can see through an org.
    /// Each page is retried on its own, so one flaky page doesn't restart the list.
    pub async fn user_repos(&self) -> Result<Vec<Repo>> {
        let mut repos = Vec::new();
        for page in 1.. {
            let page = page.to_string();
            let batch: Vec<Repo> = self
                .get(
                    "/user/repos",
                    &[
                        ("affiliation", "owner,collaborator,organization_member"),
                        ("per_page", "100"),
                        ("page", &page),
                    ],
                )
                .await
                .context("listing GitHub repositories")?;
            let done = batch.len() < 100;
            repos.extend(batch);
            if done {
                break;
            }
        }
        Ok(repos)
    }
}

/// Sort a response into success, a definite failure, or a transient one
/// (with GitHub's requested wait, if it gave one).
async fn classify(response: Response) -> Result<Response, (Failure, Option<Duration>)> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let header = |name: &str| {
        response
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    };
    let retry_after = header("retry-after")
        .and_then(|s| s.parse().ok())
        .map(Duration::from_secs);
    let rate_limited = status == StatusCode::TOO_MANY_REQUESTS
        || (status == StatusCode::FORBIDDEN
            && header("x-ratelimit-remaining").as_deref() == Some("0"));
    let body = response.text().await.unwrap_or_default();
    let failure = if status == StatusCode::NOT_FOUND {
        Failure::NotFound
    } else if rate_limited {
        Failure::Unavailable(format!("rate limited ({status})"))
    } else if status.is_server_error() {
        Failure::Unavailable(format!("{status}"))
    } else {
        Failure::Rejected(status, body)
    };
    Err((failure, retry_after))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Read, Write},
        net::TcpListener,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    /// A fake API that answers each request with the next canned status/body.
    fn fake_api(responses: Vec<(u16, &'static str)>) -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        std::thread::spawn(move || {
            for (stream, (status, body)) in listener.incoming().zip(responses) {
                let mut stream = stream.unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = v.trim().parse().unwrap();
                    }
                    if line == "\r\n" {
                        break;
                    }
                }
                reader.read_exact(&mut vec![0; length]).unwrap();
                counter.fetch_add(1, Ordering::SeqCst);
                write!(
                    stream,
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
        });
        (url, hits)
    }

    fn client(api: String) -> GitHub {
        let mut github = GitHub::new(Some("token".into()));
        github.api = api;
        github
    }

    const REPO: &str = r#"{"full_name":"o/r","clone_url":"https://github.com/o/r.git"}"#;

    #[tokio::test]
    async fn reads_retry_through_server_errors() {
        let (api, hits) = fake_api(vec![(503, ""), (502, ""), (200, REPO)]);
        let repo = client(api).patient().repo("o/r").await.unwrap();
        assert_eq!(repo.full_name, "o/r");
        assert_eq!(hits.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn gives_up_as_unavailable_after_retries() {
        let (api, hits) = fake_api(vec![(500, ""), (500, ""), (500, "")]);
        let err = client(api).repo("o/r").await.unwrap_err();
        assert!(is_unavailable(&err), "{err:#}");
        assert_eq!(hits.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn not_found_is_definite_and_not_retried() {
        let (api, hits) = fake_api(vec![(404, "{}"), (200, REPO)]);
        let err = client(api).repo("o/r").await.unwrap_err();
        assert!(is_not_found(&err));
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn writes_are_never_retried() {
        let (api, hits) = fake_api(vec![(503, ""), (200, REPO)]);
        let err = client(api)
            .patient()
            .create_repo(None, "r", None)
            .await
            .unwrap_err();
        assert!(is_unavailable(&err));
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn unreachable_host_is_unavailable() {
        let err = client("http://127.0.0.1:9".into())
            .repo("o/r")
            .await
            .unwrap_err();
        assert!(is_unavailable(&err), "{err:#}");
    }
}
