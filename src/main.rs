mod cli;
mod config;
mod db;
mod github_auth;
mod project;

use anyhow::{Context, Result, bail};
use clap::Parser;
use cli::{Cli, Commands, ConfigCmd, GithubCmd, OrgCmd, Shell};
use colored::{Color, Colorize};
use inquire::Confirm;
use project::Project;
use regex::Regex;
use serde::Deserialize;
use skim::{prelude::*, tui::BorderType};
use sqlx::SqlitePool;
use std::{
    collections::BTreeMap,
    env, fs,
    io::Cursor,
    path::{Path, PathBuf},
    process::Command,
};

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("{} {e:#}", "error:".red().bold());
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let cli = Cli::parse();
    if let Some(Commands::Hook { shell }) = &cli.command {
        hook_cmd(*shell);
        return Ok(());
    }
    if let Some(Commands::HookInstall { shell }) = &cli.command {
        install_hook_cmd(*shell)?;
        return Ok(());
    }

    let pool = db::connect().await?;
    db::migrate(&pool).await?;

    match cli.command {
        Some(Commands::Resolve { name }) => {
            if let Some(p) = db::find_project(&pool, &name).await? {
                let p = ensure_local_project(&pool, p).await?;
                print!("{}", p.path);
            } else if let Some(p) = maybe_clone_github_repo(&pool, &name).await? {
                print!("{}", p.path);
            } else {
                std::process::exit(1);
            }
        }
        Some(Commands::Create {
            name,
            public,
            private: _,
            description,
        }) => create_cmd(&pool, &name, public, description).await?,
        Some(Commands::List {
            source,
            scope,
            json,
        }) => list(&pool, source, scope, json).await?,
        Some(Commands::Remove { path, force }) => remove_cmd(path, force)?,
        Some(Commands::Dev) => dev_cmd()?,
        Some(Commands::Config { command }) => config_cmd(command)?,
        Some(Commands::Org { command }) => org_cmd(&pool, command).await?,
        Some(Commands::Github { command }) => github_cmd(command).await?,
        Some(Commands::Hook { shell }) => hook_cmd(shell),
        Some(Commands::HookInstall { shell }) => install_hook_cmd(shell)?,
        Some(Commands::Index { path, quiet }) => index_cmd(&pool, path, quiet).await?,
        None => select_cmd(&pool, cli).await?,
    }
    Ok(())
}

async fn select_cmd(db: &SqlitePool, cli: Cli) -> Result<()> {
    let ps = db::projects(db).await?;
    if cli.json {
        println!("{}", serde_json::to_string(&ps)?);
        return Ok(());
    }

    let p = if let Some(n) = cli.name {
        match db::find_project(db, &n).await? {
            Some(p) => ensure_local_project(db, p).await?,
            None => match maybe_clone_github_repo(db, &n).await? {
                Some(p) => p,
                None => {
                    eprintln!(
                        "{} no project found for {}",
                        "warning:".yellow().bold(),
                        n.bold()
                    );
                    let Some(p) = choose(ps, Some(&n))? else {
                        return Ok(());
                    };
                    ensure_local_project(db, p).await?
                }
            },
        }
    } else {
        let Some(p) = choose(ps, None)? else {
            return Ok(());
        };
        ensure_local_project(db, p).await?
    };
    db::touch(db, &p).await?;

    if cli.path || cli.silent {
        if cli.path {
            print!("{}", p.path);
        }
    } else if cli.agent {
        spawn_agent_in(&p.path)?;
    } else if let Some(cmd) = cli.open {
        Command::new(cmd).arg(&p.path).status()?;
    } else if let Some(app) = cli.app {
        Command::new("open").args(["-a", &app, &p.path]).status()?;
    } else {
        spawn_shell_in(&p.path)?;
    }
    Ok(())
}

async fn ensure_local_project(db: &SqlitePool, project: Project) -> Result<Project> {
    if let Some(name) = project.path.strip_prefix("github://") {
        return maybe_clone_github_repo(db, name)
            .await?
            .with_context(|| format!("{} is not available locally", project.path));
    }

    if Path::new(&project.path).exists() {
        return Ok(project);
    }

    if let Some(full_name) = project
        .github_full_name
        .as_deref()
        .map(str::to_owned)
        .or_else(|| infer_github_full_name_from_path(&project.path))
    {
        db::mark_project_remote_only(db, &project.id, &full_name).await?;
        return maybe_clone_github_repo(db, &full_name)
            .await?
            .with_context(|| format!("{full_name} is not available locally"));
    }

    Ok(project)
}

fn infer_github_full_name_from_path(path: &str) -> Option<String> {
    let path = Path::new(path);
    let repo = path.file_name()?.to_str()?;
    let owner = path.parent()?.file_name()?.to_str()?;
    Some(format!("{owner}/{repo}"))
}

#[derive(Debug, Deserialize)]
struct GitHubRepoResponse {
    full_name: String,
    clone_url: String,
}

#[derive(Debug, Deserialize)]
struct GitHubRepoListItem {
    full_name: String,
}

async fn create_cmd(
    db: &SqlitePool,
    name: &str,
    public: bool,
    description: Option<String>,
) -> Result<()> {
    if public {
        bail!(
            "creating public repositories with p is disabled; create public repositories manually"
        );
    }

    let token = github_auth::load_token()?
        .context("GitHub authentication required; run `p github login` or `gh auth login` first")?;
    let login = github_auth::validate_token(Some(&token))
        .await?
        .context("GitHub authentication is invalid; run `p github login` or `gh auth login`")?;

    let (owner, repo) = if let Some((owner, repo)) = name.split_once('/') {
        (owner.to_string(), repo.to_string())
    } else {
        (login.clone(), name.to_string())
    };
    validate_github_repo_parts(&owner, &repo)?;

    let target = github_checkout_path(&owner, &repo)?;
    if target.exists() {
        bail!("local target already exists: {}", target.display());
    }

    let url = if owner.eq_ignore_ascii_case(&login) {
        "https://api.github.com/user/repos".to_string()
    } else {
        format!("https://api.github.com/orgs/{owner}/repos")
    };
    let body = serde_json::json!({
        "name": repo,
        "private": !public,
        "description": description,
    });
    let response = reqwest::Client::new()
        .post(url)
        .header(reqwest::header::USER_AGENT, "pm")
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .bearer_auth(&token)
        .json(&body)
        .send()
        .await
        .context("failed to call GitHub repository create API")?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        bail!("failed to create GitHub repository {owner}/{repo}: {status} {body}");
    }
    let repo_info = response
        .json::<GitHubRepoResponse>()
        .await
        .context("failed to parse GitHub repository create response")?;

    git_clone(
        &repo_info.full_name,
        &repo_info.clone_url,
        &target,
        Some(&token),
    )?;
    db::upsert_project(db, &target).await?;
    println!("{}", target.display());
    Ok(())
}

fn validate_github_repo_parts(owner: &str, repo: &str) -> Result<()> {
    if owner.is_empty() || repo.is_empty() || repo.contains('/') {
        bail!("invalid GitHub repository name: {owner}/{repo}");
    }
    Ok(())
}

async fn maybe_clone_github_repo(db: &SqlitePool, name: &str) -> Result<Option<Project>> {
    let Some((owner, repo)) = name.split_once('/') else {
        return Ok(None);
    };
    if owner.is_empty() || repo.is_empty() || repo.contains('/') {
        return Ok(None);
    }

    let github = github_auth::client()?;
    let token = github_auth::load_token()?;
    let repo_info = github.repos(owner, repo).get().await.with_context(|| {
        let auth_hint = if token.is_some() {
            "check that your token has private-repository access and org SSO authorization"
        } else {
            "authenticate with `p github login` or `gh auth login` to access private repositories"
        };
        format!("GitHub repository not found or inaccessible: {owner}/{repo}; {auth_hint}")
    })?;
    let full_name = repo_info
        .full_name
        .clone()
        .unwrap_or_else(|| format!("{owner}/{repo}"));
    let target = github_checkout_path(owner, repo)?;

    if target.exists() {
        db::upsert_project(db, &target).await?;
        return db::find_project(db, repo).await;
    }

    let prompt = format!(
        "{full_name} is not available locally. Clone it to {}?",
        target.display()
    );
    if !Confirm::new(&prompt).with_default(true).prompt()? {
        return Ok(None);
    }

    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    let clone_url = repo_info
        .clone_url
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("https://github.com/{full_name}.git"));
    let mut clone = Command::new("git");
    clone.arg("clone");
    if let Some(token) = token.as_deref() {
        clone
            .env("GIT_CONFIG_COUNT", "1")
            .env("GIT_CONFIG_KEY_0", "http.extraHeader")
            .env(
                "GIT_CONFIG_VALUE_0",
                format!("Authorization: Bearer {token}"),
            );
    }
    let status = clone
        .arg(&clone_url)
        .arg(&target)
        .status()
        .context("failed to run git clone")?;
    if !status.success() {
        bail!("git clone failed for {full_name}");
    }

    db::upsert_project(db, &target).await?;
    db::find_project(db, repo)
        .await?
        .with_context(|| format!("cloned {full_name}, but could not index it"))
        .map(Some)
}

fn git_clone(full_name: &str, clone_url: &str, target: &Path, token: Option<&str>) -> Result<()> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut clone = Command::new("git");
    clone.arg("clone");
    if let Some(token) = token {
        clone
            .env("GIT_CONFIG_COUNT", "1")
            .env("GIT_CONFIG_KEY_0", "http.extraHeader")
            .env(
                "GIT_CONFIG_VALUE_0",
                format!("Authorization: Bearer {token}"),
            );
    }
    let status = clone
        .arg(clone_url)
        .arg(target)
        .status()
        .context("failed to run git clone")?;
    if !status.success() {
        bail!("git clone failed for {full_name}");
    }
    Ok(())
}

fn github_checkout_path(owner: &str, repo: &str) -> Result<PathBuf> {
    let home = directories::BaseDirs::new()
        .context("home dir unavailable")?
        .home_dir()
        .to_path_buf();
    Ok(home.join("Documents").join(owner).join(repo))
}

fn style_scope(scope: &str) -> colored::ColoredString {
    scope.bold().color(Color::BrightCyan)
}

fn style_name(name: &str) -> colored::ColoredString {
    name.bold().color(Color::BrightGreen)
}

fn style_path(path: &str) -> colored::ColoredString {
    path.dimmed()
}

fn choose(ps: Vec<Project>, initial_query: Option<&str>) -> Result<Option<Project>> {
    if ps.is_empty() {
        bail!("no projects found")
    }

    let input = ps
        .iter()
        .map(|p| format!("{}\t{}\t{}", style_name(&p.name), style_path(&p.path), p.id))
        .collect::<Vec<_>>()
        .join("\n");

    let mut options_builder = SkimOptionsBuilder::default();
    options_builder
        .prompt(format!("{} ", "Select project>".bright_cyan().bold()))
        .height("80%")
        .reverse(true)
        .border(BorderType::Rounded)
        .with_nth(vec!["1".to_string(), "2".to_string()])
        .delimiter(Regex::new("\t").context("failed to configure project selector delimiter")?);
    if let Some(query) = initial_query {
        options_builder.query(query.to_string());
    }
    let options = options_builder
        .build()
        .context("failed to configure project selector")?;
    let item_reader = SkimItemReader::new(SkimItemReaderOption::default().ansi(true));
    let items = item_reader.of_bufread(Cursor::new(input));
    let output = Skim::run_with(options, Some(items))
        .map_err(|err| anyhow::anyhow!("failed to run project selector: {err}"))?;
    if output.is_abort {
        return Ok(None);
    }
    let Some(selected) = output.selected_items.first() else {
        return Ok(None);
    };
    let selected_output = selected.output();
    let id = selected_output
        .rsplit('\t')
        .next()
        .context("project selector returned an invalid selection")?;

    ps.into_iter()
        .find(|p| p.id == id)
        .context("selected project no longer exists")
        .map(Some)
}

fn spawn_agent_in(path: &str) -> Result<()> {
    Command::new("pi")
        .current_dir(path)
        .status()
        .context("failed to launch pi")?;
    Ok(())
}

fn spawn_shell_in(path: &str) -> Result<()> {
    let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    let shell_path = PathBuf::from(&shell);
    let shell_name = shell_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("sh");

    let mut command = Command::new(&shell);
    if shell_name == "fish" {
        command
            .arg("-c")
            .arg("cd $argv[1]; and exec $argv[2]")
            .arg(path)
            .arg(&shell);
    } else {
        command
            .arg("-c")
            .arg("cd \"$1\" && exec \"$2\" -l")
            .arg("p")
            .arg(path)
            .arg(&shell);
    }
    command.status()?;
    Ok(())
}

async fn list(
    db: &SqlitePool,
    source: Option<String>,
    scope: Option<String>,
    json: bool,
) -> Result<()> {
    let mut ps = db::projects(db).await?;
    if let Some(s) = source {
        ps.retain(|p| p.source == s);
    }
    if let Some(s) = scope {
        ps.retain(|p| p.scope.as_deref() == Some(&s));
    }
    if json {
        println!("{}", serde_json::to_string(&ps)?);
    } else {
        let mut groups: BTreeMap<String, Vec<Project>> = BTreeMap::new();
        for p in ps {
            groups
                .entry(p.scope.clone().unwrap_or("unscoped".into()))
                .or_default()
                .push(p);
        }
        for (g, items) in groups {
            println!("{}", style_scope(&g));
            for p in items {
                println!("  {}  {}", style_name(&p.name), style_path(&p.path));
            }
        }
    }
    Ok(())
}

fn remove_cmd(path: Option<String>, force: bool) -> Result<()> {
    let p = path.map(PathBuf::from).unwrap_or(env::current_dir()?);
    if !force {
        eprintln!(
            "{} refusing to delete without {}: {}",
            "warning:".yellow().bold(),
            "--force".bold(),
            p.display().to_string().dimmed()
        );
        return Ok(());
    }
    fs::remove_dir_all(&p)?;
    Ok(())
}

fn dev_cmd() -> Result<()> {
    let cwd = env::current_dir()?;
    for (file, cmd, args) in [
        ("bun.lock", "bun", vec!["run", "dev"]),
        ("package.json", "npm", vec!["run", "dev"]),
        ("Cargo.toml", "cargo", vec!["run"]),
    ] {
        if cwd.join(file).exists() {
            Command::new(cmd).args(args).current_dir(cwd).status()?;
            return Ok(());
        }
    }
    bail!("could not detect dev command")
}

fn config_cmd(cmd: ConfigCmd) -> Result<()> {
    let path = config::config_path()?;
    let mut v: serde_json::Value = fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::json!({}));
    match cmd {
        ConfigCmd::List => println!("{}", serde_json::to_string_pretty(&v)?),
        ConfigCmd::Set { key, value } => {
            v[&key] = serde_json::Value::String(value);
            fs::write(path, serde_json::to_string_pretty(&v)?)?
        }
        ConfigCmd::Delete { key } => {
            v.as_object_mut().map(|o| o.remove(&key));
            fs::write(path, serde_json::to_string_pretty(&v)?)?
        }
    }
    Ok(())
}

async fn org_cmd(db: &SqlitePool, cmd: OrgCmd) -> Result<()> {
    match cmd {
        OrgCmd::List => {
            for (n, h) in db::orgs(db).await? {
                if h != 0 {
                    println!("{} {}", style_name(&n), "(hidden)".yellow());
                } else {
                    println!("{}", style_name(&n));
                }
            }
        }
        OrgCmd::Hide { name } => db::set_org_hidden(db, name, true).await?,
        OrgCmd::Show { name } => db::set_org_hidden(db, name, false).await?,
    }
    Ok(())
}

async fn github_cmd(cmd: GithubCmd) -> Result<()> {
    match cmd {
        GithubCmd::Login { client_id } => github_auth::login(client_id).await?,
        GithubCmd::Status => github_auth::status().await?,
        GithubCmd::Logout => github_auth::logout()?,
    }
    Ok(())
}

async fn index_cmd(db: &SqlitePool, path: Option<String>, quiet: bool) -> Result<()> {
    if let Some(path) = path {
        let path = PathBuf::from(path);
        let Some(root) = db::git_repo_root(&path)? else {
            if !quiet {
                eprintln!("{} not inside a git repository", "warning:".yellow().bold());
            }
            return Ok(());
        };
        db::upsert_project(db, &root).await?;
        if !quiet {
            println!("{} {}", "indexed".green().bold(), root.display());
        }
        return Ok(());
    }

    let local = db::scan(db).await?;
    let remote = index_remote_repos(db, quiet).await?;
    if !quiet {
        println!(
            "{} {} local, {} remote",
            "indexed".green().bold(),
            local,
            remote
        );
    }
    Ok(())
}

async fn index_remote_repos(db: &SqlitePool, quiet: bool) -> Result<usize> {
    let Some(token) = github_auth::load_token()? else {
        if !quiet {
            eprintln!(
                "{} remote indexing skipped; authenticate with {} first",
                "warning:".yellow().bold(),
                "p github login".bold()
            );
        }
        return Ok(0);
    };

    let http = reqwest::Client::new();
    let mut indexed = 0;
    let mut page = 1;
    loop {
        let repos = http
            .get("https://api.github.com/user/repos")
            .query(&[
                ("affiliation", "owner,collaborator,organization_member"),
                ("per_page", "100"),
                ("page", &page.to_string()),
            ])
            .header(reqwest::header::USER_AGENT, "pm")
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .bearer_auth(&token)
            .send()
            .await
            .context("failed to list GitHub repositories")?
            .error_for_status()
            .context("failed to list GitHub repositories")?
            .json::<Vec<GitHubRepoListItem>>()
            .await
            .context("failed to parse GitHub repository list")?;
        let count = repos.len();
        for repo in repos {
            db::upsert_remote_project(db, &repo.full_name).await?;
            indexed += 1;
        }
        if count < 100 {
            break;
        }
        page += 1;
    }
    Ok(indexed)
}

const ZSH_HOOK: &str = r#"# p: index git repositories after git creates them.
_p_index_git_repo_at() {
  emulate -L zsh
  command -v p >/dev/null 2>&1 || return
  [[ -n "$1" ]] || return
  p index --quiet "$1" >/dev/null 2>&1 &!
}
_p_git_non_option_args() {
  emulate -L zsh
  local -a positional
  local arg skip_next=0 after_options=0
  for arg in "$@"; do
    if (( skip_next )); then
      skip_next=0
      continue
    fi
    if (( after_options )); then
      positional+=("$arg")
      continue
    fi
    case "$arg" in
      --) after_options=1 ;;
      -b|--branch|-o|--origin|--depth|--config|-c|--jobs|-j|--server-option|--reference|--reference-if-able|--separate-git-dir|--template|-u|--upload-pack) skip_next=1 ;;
      -*) ;;
      *) positional+=("$arg") ;;
    esac
  done
  (( ${#positional} )) && printf '%s\n' "${positional[@]}"
}
_p_git_clone_dest() {
  emulate -L zsh
  local -a positional
  positional=(${(f)"$(_p_git_non_option_args "$@")"})
  (( ${#positional} )) || return
  if (( ${#positional} >= 2 )); then
    print -r -- "$positional[2]"
  else
    local repo="$positional[1]" dest
    dest="${repo:t}"
    print -r -- "${dest%.git}"
  fi
}
git() {
  command git "$@"
  local git_status=$?
  (( git_status == 0 )) || return $git_status
  case "$1" in
    init)
      local -a positional
      positional=(${(f)"$(_p_git_non_option_args "${@:2}")"})
      if (( ${#positional} )); then
        _p_index_git_repo_at "$positional[-1]"
      else
        _p_index_git_repo_at "$PWD"
      fi
      ;;
    clone)
      local dest
      dest="$(_p_git_clone_dest "${@:2}")"
      [[ -n "$dest" ]] && _p_index_git_repo_at "$dest"
      ;;
    worktree)
      if [[ "$2" == "add" ]]; then
        local -a positional
        positional=(${(f)"$(_p_git_non_option_args "${@:3}")"})
        (( ${#positional} )) && _p_index_git_repo_at "$positional[1]"
      fi
      ;;
  esac
  return $git_status
}
"#;

const BASH_HOOK: &str = r#"# p: index git repositories after git creates them.
__p_index_git_repo_at() {
  command -v p >/dev/null 2>&1 || return
  [ -n "$1" ] || return
  p index --quiet "$1" >/dev/null 2>&1 &
}
__p_git_non_option_args() {
  local positional=() arg skip_next=0 after_options=0
  for arg in "$@"; do
    if [ "$skip_next" -eq 1 ]; then
      skip_next=0
      continue
    fi
    if [ "$after_options" -eq 1 ]; then
      positional+=("$arg")
      continue
    fi
    case "$arg" in
      --) after_options=1 ;;
      -b|--branch|-o|--origin|--depth|--config|-c|--jobs|-j|--server-option|--reference|--reference-if-able|--separate-git-dir|--template|-u|--upload-pack) skip_next=1 ;;
      -*) ;;
      *) positional+=("$arg") ;;
    esac
  done
  [ "${#positional[@]}" -gt 0 ] && printf '%s\n' "${positional[@]}"
}
__p_git_clone_dest() {
  local positional=() repo dest
  while IFS= read -r line; do
    positional+=("$line")
  done < <(__p_git_non_option_args "$@")
  [ "${#positional[@]}" -gt 0 ] || return
  if [ "${#positional[@]}" -ge 2 ]; then
    printf '%s\n' "${positional[1]}"
  else
    repo="${positional[0]}"
    dest="${repo##*/}"
    printf '%s\n' "${dest%.git}"
  fi
}
git() {
  command git "$@"
  local status=$?
  [ "$status" -eq 0 ] || return "$status"
  case "$1" in
    init)
      local positional=()
      while IFS= read -r line; do
        positional+=("$line")
      done < <(__p_git_non_option_args "${@:2}")
      if [ "${#positional[@]}" -gt 0 ]; then
        __p_index_git_repo_at "${positional[$((${#positional[@]} - 1))]}"
      else
        __p_index_git_repo_at "$PWD"
      fi
      ;;
    clone)
      local dest
      dest="$(__p_git_clone_dest "${@:2}")"
      [ -n "$dest" ] && __p_index_git_repo_at "$dest"
      ;;
    worktree)
      if [ "$2" = "add" ]; then
        local positional=()
        while IFS= read -r line; do
          positional+=("$line")
        done < <(__p_git_non_option_args "${@:3}")
        [ "${#positional[@]}" -gt 0 ] && __p_index_git_repo_at "${positional[0]}"
      fi
      ;;
  esac
  return "$status"
}
"#;

const FISH_HOOK: &str = r#"# p: index git repositories after git creates them.
function __p_index_git_repo_at
  command -q p; or return
  test -n "$argv[1]"; or return
  p index --quiet "$argv[1]" >/dev/null 2>&1 &
end
function __p_git_non_option_args
  set -l positional
  set -l skip_next 0
  set -l after_options 0
  for arg in $argv
    if test $skip_next -eq 1
      set skip_next 0
      continue
    end
    if test $after_options -eq 1
      set -a positional "$arg"
      continue
    end
    switch "$arg"
      case --
        set after_options 1
      case -b --branch -o --origin --depth --config -c --jobs -j --server-option --reference --reference-if-able --separate-git-dir --template -u --upload-pack
        set skip_next 1
      case '-*'
      case '*'
        set -a positional "$arg"
    end
  end
  test (count $positional) -gt 0; and printf '%s\n' $positional
end
function __p_git_clone_dest
  set -l positional (__p_git_non_option_args $argv)
  test (count $positional) -gt 0; or return
  if test (count $positional) -ge 2
    printf '%s\n' $positional[2]
  else
    set -l repo $positional[1]
    set -l dest (basename "$repo")
    string replace -r '\.git$' '' -- "$dest"
  end
end
function git
  command git $argv
  set -l git_status $status
  test $git_status -eq 0; or return $git_status
  switch "$argv[1]"
    case init
      set -l positional (__p_git_non_option_args $argv[2..-1])
      if test (count $positional) -gt 0
        __p_index_git_repo_at $positional[-1]
      else
        __p_index_git_repo_at "$PWD"
      end
    case clone
      set -l dest (__p_git_clone_dest $argv[2..-1])
      test -n "$dest"; and __p_index_git_repo_at "$dest"
    case worktree
      if test "$argv[2]" = add
        set -l positional (__p_git_non_option_args $argv[3..-1])
        test (count $positional) -gt 0; and __p_index_git_repo_at $positional[1]
      end
  end
  return $git_status
end
"#;

fn hook_text(shell: Shell) -> &'static str {
    match shell {
        Shell::Zsh => ZSH_HOOK,
        Shell::Bash => BASH_HOOK,
        Shell::Fish => FISH_HOOK,
    }
}

fn hook_cmd(shell: Shell) {
    print!("{}", hook_text(shell));
}

fn install_hook_cmd(shell: Option<Shell>) -> Result<()> {
    let shell = shell.unwrap_or_else(detect_shell);
    let path = hook_install_path(shell)?;
    let hook = format!(
        "# >>> p hook >>>\n{}\n# <<< p hook <<<\n",
        hook_text(shell).trim_end_matches('\n')
    );

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let existing = fs::read_to_string(&path).unwrap_or_default();
    let updated = replace_or_append_hook(&existing, &hook);
    fs::write(&path, updated)?;
    println!(
        "{} installed {} hook in {}",
        "ok:".green().bold(),
        shell_name(shell).bold(),
        path.display().to_string().dimmed()
    );
    Ok(())
}

fn detect_shell() -> Shell {
    let shell = env::var("SHELL").unwrap_or_default();
    let shell_name = Path::new(&shell)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    match shell_name {
        "bash" => Shell::Bash,
        "fish" => Shell::Fish,
        _ => Shell::Zsh,
    }
}

fn hook_install_path(shell: Shell) -> Result<PathBuf> {
    let home = directories::BaseDirs::new()
        .context("home dir unavailable")?
        .home_dir()
        .to_path_buf();
    Ok(match shell {
        Shell::Zsh => home.join(".zshrc"),
        Shell::Bash => home.join(".bashrc"),
        Shell::Fish => home
            .join(".config")
            .join("fish")
            .join("conf.d")
            .join("p.fish"),
    })
}

fn replace_or_append_hook(existing: &str, hook: &str) -> String {
    let start = "# >>> p hook >>>";
    let end = "# <<< p hook <<<";
    if let Some(start_idx) = existing.find(start)
        && let Some(relative_end_idx) = existing[start_idx..].find(end)
    {
        let end_idx = start_idx + relative_end_idx + end.len();
        let mut updated = String::new();
        updated.push_str(&existing[..start_idx]);
        updated.push_str(hook);
        updated.push_str(existing[end_idx..].trim_start_matches('\n'));
        return updated;
    }

    let mut updated = existing.to_string();
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    if !updated.is_empty() {
        updated.push('\n');
    }
    updated.push_str(hook);
    updated
}

fn shell_name(shell: Shell) -> &'static str {
    match shell {
        Shell::Zsh => "zsh",
        Shell::Bash => "bash",
        Shell::Fish => "fish",
    }
}
