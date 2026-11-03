use crate::github::GitHub;
use anyhow::{Context, Result, bail};
use colored::Colorize;
use directories::BaseDirs;
use serde::Deserialize;
use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    time::Duration,
};
use tokio::time::sleep;

const DEFAULT_SCOPES: &str = "repo read:org";

#[derive(Debug, Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    interval: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct AccessTokenResponse {
    access_token: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

pub fn token_path() -> Result<PathBuf> {
    let dir = BaseDirs::new()
        .context("home dir unavailable")?
        .config_dir()
        .join("pm");
    fs::create_dir_all(&dir)?;
    Ok(dir.join("github-token"))
}

#[derive(Debug)]
pub struct Token {
    value: String,
    source: String,
}

impl Token {
    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn source(&self) -> &str {
        &self.source
    }
}

pub fn load_token() -> Result<Option<String>> {
    Ok(load_token_with_source()?.map(|token| token.value))
}

pub fn load_token_with_source() -> Result<Option<Token>> {
    let path = token_path()?;
    if let Some(token) = read_token_file(&path)? {
        return Ok(Some(Token {
            value: token,
            source: path.display().to_string(),
        }));
    }

    for name in ["PM_GITHUB_TOKEN", "GH_TOKEN", "GITHUB_TOKEN"] {
        if let Some(token) = std::env::var(name)
            .ok()
            .map(|token| token.trim().to_string())
            .filter(|token| !token.is_empty())
        {
            return Ok(Some(Token {
                value: token,
                source: format!("${name}"),
            }));
        }
    }

    if let Some(token) = gh_auth_token() {
        return Ok(Some(Token {
            value: token,
            source: "gh auth token".into(),
        }));
    }

    Ok(None)
}

fn read_token_file(path: &PathBuf) -> Result<Option<String>> {
    Ok(fs::read_to_string(path)
        .ok()
        .map(|token| token.trim().to_string())
        .filter(|token| !token.is_empty()))
}

fn gh_auth_token() -> Option<String> {
    let output = Command::new("gh")
        .args(["auth", "token"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()
        .map(|token| token.trim().to_string())
        .filter(|token| !token.is_empty())
}

pub fn save_token(token: &str) -> Result<()> {
    let path = token_path()?;
    fs::write(&path, token.trim())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

pub fn logout() -> Result<()> {
    let path = token_path()?;
    match fs::remove_file(&path) {
        Ok(()) => println!("removed GitHub credentials from {}", path.display()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            println!("no GitHub credentials saved at {}", path.display())
        }
        Err(e) => return Err(e).context("failed to remove GitHub credentials"),
    }
    Ok(())
}

pub async fn login(client_id: Option<String>) -> Result<()> {
    let client_id = client_id
        .or_else(|| std::env::var("PM_GITHUB_CLIENT_ID").ok())
        .filter(|value| !value.trim().is_empty())
        .context("GitHub OAuth device login requires --client-id or PM_GITHUB_CLIENT_ID")?;

    let http = reqwest::Client::new();
    let device = http
        .post("https://github.com/login/device/code")
        .header(reqwest::header::ACCEPT, "application/json")
        .form(&[("client_id", client_id.as_str()), ("scope", DEFAULT_SCOPES)])
        .send()
        .await?
        .error_for_status()?
        .json::<DeviceCodeResponse>()
        .await?;

    println!(
        "Open {} and enter code {}",
        device.verification_uri.bold(),
        device.user_code.bold().bright_green()
    );
    let _ = Command::new("open").arg(&device.verification_uri).status();

    let started = std::time::Instant::now();
    let mut interval = device.interval.unwrap_or(5);
    loop {
        if started.elapsed() > Duration::from_secs(device.expires_in) {
            bail!("GitHub login expired before authorization completed");
        }

        sleep(Duration::from_secs(interval)).await;
        let response = http
            .post("https://github.com/login/oauth/access_token")
            .header(reqwest::header::ACCEPT, "application/json")
            .form(&[
                ("client_id", client_id.as_str()),
                ("device_code", device.device_code.as_str()),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ])
            .send()
            .await?
            .error_for_status()?
            .json::<AccessTokenResponse>()
            .await?;

        if let Some(token) = response.access_token {
            save_token(&token)?;
            let login = GitHub::new(Some(token))
                .current_user()
                .await?
                .unwrap_or_else(|| "GitHub".into());
            println!("authenticated as {}", login.bold().bright_green());
            return Ok(());
        }

        match response.error.as_deref() {
            Some("authorization_pending") => {}
            Some("slow_down") => interval += 5,
            Some("expired_token") => bail!("GitHub login code expired"),
            Some("access_denied") => bail!("GitHub login was denied"),
            Some(error) => bail!(
                "GitHub login failed: {}",
                response
                    .error_description
                    .unwrap_or_else(|| error.to_string())
            ),
            None => bail!("GitHub login failed without an access token"),
        }
    }
}

pub async fn status() -> Result<()> {
    let path = token_path()?;
    let token = load_token_with_source()?;
    let github = GitHub::new(token.as_ref().map(|t| t.value().to_string()));
    match github.current_user().await? {
        Some(login) => println!(
            "authenticated as {} ({})",
            login.bold().bright_green(),
            token
                .as_ref()
                .map(Token::source)
                .unwrap_or_else(|| path.to_str().unwrap_or("unknown source"))
        ),
        None => println!("not authenticated ({})", path.display()),
    }
    Ok(())
}

/// The saved token; if there is none and `gh` is installed, offer `gh auth login`.
pub fn token_or_login() -> Result<Option<String>> {
    let token = load_token()?;
    if token.is_some() || !crate::ui::can_prompt_interactively() || !gh_cli_available() {
        return Ok(token);
    }
    if !crate::ui::confirm_yes_no("No GitHub token found. Run `gh auth login` now?", true)? {
        return Ok(None);
    }
    let status = Command::new("gh")
        .args(["auth", "login"])
        .status()
        .context("failed to run gh auth login")?;
    if !status.success() {
        bail!("gh auth login failed");
    }
    load_token()
}

/// The saved token, or an error telling the user how to log in.
pub fn require_token() -> Result<String> {
    token_or_login()?
        .context("GitHub authentication required; run `gh auth login` or `p github login` first")
}

fn gh_cli_available() -> bool {
    Command::new("gh")
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}
