mod cli;
mod config;
mod db;
mod github_auth;
mod project;

use anyhow::{Context, Result, bail};
use clap::Parser;
use cli::{Cli, Commands, ConfigCmd, GithubCmd, OrgCmd};
use colored::{Color, Colorize};
use inquire::Confirm;
use project::Project;
use regex::Regex;
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

async fn maybe_clone_github_repo(db: &SqlitePool, name: &str) -> Result<Option<Project>> {
    let Some((owner, repo)) = name.split_once('/') else {
        return Ok(None);
    };
    if owner.is_empty() || repo.is_empty() || repo.contains('/') {
        return Ok(None);
    }

    let github = github_auth::client()?;
    let token = github_auth::load_token()?;
    let repo_info = github
        .repos(owner, repo)
        .get()
        .await
        .with_context(|| format!("GitHub repository not found: {owner}/{repo}"))?;
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
