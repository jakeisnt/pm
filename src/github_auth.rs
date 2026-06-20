use anyhow::{Context, Result, bail};
use colored::Colorize;
use directories::BaseDirs;
use serde::Deserialize;
use std::{fs, path::PathBuf, process::Command, time::Duration};
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

#[derive(Debug, Deserialize)]
struct UserResponse {
    login: String,
}

pub fn token_path() -> Result<PathBuf> {
    let dir = BaseDirs::new()
        .context("home dir unavailable")?
        .config_dir()
        .join("pm");
    fs::create_dir_all(&dir)?;
    Ok(dir.join("github-token"))
}

pub fn load_token() -> Result<Option<String>> {
    let path = token_path()?;
    Ok(fs::read_to_string(path)
        .ok()
        .map(|token| token.trim().to_string())
        .filter(|token| !token.is_empty()))
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
            let login = validate_token(Some(&token))
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
    match validate_token(load_token()?.as_deref()).await? {
        Some(login) => println!(
            "authenticated as {} ({})",
            login.bold().bright_green(),
            path.display()
        ),
        None => println!("not authenticated ({})", path.display()),
    }
    Ok(())
}

pub async fn validate_token(token: Option<&str>) -> Result<Option<String>> {
    let Some(token) = token else {
        return Ok(None);
    };
    let response = reqwest::Client::new()
        .get("https://api.github.com/user")
        .header(reqwest::header::USER_AGENT, "pm")
        .bearer_auth(token)
        .send()
        .await?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Ok(None);
    }
    let user = response.error_for_status()?.json::<UserResponse>().await?;
    Ok(Some(user.login))
}

pub fn client() -> Result<octocrab::Octocrab> {
    if let Some(token) = load_token()? {
        return octocrab::Octocrab::builder()
            .personal_token(token)
            .build()
            .context("failed to build authenticated GitHub client");
    }

    octocrab::Octocrab::builder()
        .build()
        .context("failed to build GitHub client")
}
