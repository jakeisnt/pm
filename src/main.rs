mod cli;
mod config;
mod db;
mod git;
mod github;
mod github_auth;
mod project;
mod shell;
mod ui;
mod worktree;

use anyhow::{Context, Result, bail};
use clap::Parser;
use cli::{Cli, Commands, ConfigCmd, GithubCmd, OrgCmd};
use colored::Colorize;
use github::GitHub;
use project::Project;
use sqlx::SqlitePool;
use std::{
    collections::BTreeMap,
    env, fs,
    io::{self, IsTerminal},
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
    // Commands that don't touch the project index.
    match cli.command {
        Some(Commands::Hook { shell }) => {
            shell::print_hook(shell);
            return Ok(());
        }
        Some(Commands::HookInstall { shell }) => return shell::install_hook(shell),
        Some(Commands::Worktree { project, command }) => {
            return worktree_cmd(project, command).await;
        }
        _ => {}
    }

    let pool = db::connect().await?;
    db::migrate(&pool).await?;

    match cli.command {
        Some(Commands::Resolve { name }) => match project::resolve(&pool, &name).await? {
            Some(p) => print!("{}", p.path),
            None => std::process::exit(1),
        },
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
        Some(Commands::Remove { path, force }) => remove_cmd(&pool, path, force).await?,
        Some(Commands::Rename { old_name, new_name }) => {
            rename_cmd(&pool, &old_name, &new_name).await?
        }
        Some(Commands::Dev) => dev_cmd()?,
        Some(Commands::Config { command }) => config_cmd(command)?,
        Some(Commands::Org { command }) => org_cmd(&pool, command).await?,
        Some(Commands::Github { command }) => github_cmd(command).await?,
        Some(Commands::Index { path, quiet }) => index_cmd(&pool, path, quiet).await?,
        Some(Commands::Hook { .. } | Commands::HookInstall { .. } | Commands::Worktree { .. }) => {
            unreachable!("handled before connecting to the database")
        }
        None => select_cmd(&pool, cli).await?,
    }
    Ok(())
}

async fn select_cmd(db: &SqlitePool, cli: Cli) -> Result<()> {
    if cli.json {
        println!("{}", serde_json::to_string(&db::projects(db).await?)?);
        return Ok(());
    }
    let Some(p) = project::resolve_or_pick(db, cli.name.as_deref()).await? else {
        return Ok(());
    };
    db::touch(db, &p).await?;

    if cli.path || cli.silent {
        if cli.path {
            print!("{}", p.path);
        }
    } else if cli.agent {
        ui::spawn_agent_in(&p.path)?;
    } else if let Some(cmd) = cli.open {
        Command::new(cmd).arg(&p.path).status()?;
    } else if let Some(app) = cli.app {
        Command::new("open").args(["-a", &app, &p.path]).status()?;
    } else {
        ui::spawn_shell_in(&p.path)?;
    }
    Ok(())
}

/// Run a worktree command in the named project, or in the project that
/// contains the current directory.
async fn worktree_cmd(project: Option<String>, command: Option<cli::WorktreeCmd>) -> Result<()> {
    if let Some(name) = project {
        let db = db::connect().await?;
        db::migrate(&db).await?;
        let found = if io::stdin().is_terminal() {
            project::resolve_or_pick(&db, Some(&name)).await?
        } else {
            project::resolve(&db, &name).await?
        };
        let Some(project) = found else {
            bail!("no project found for {name}");
        };
        env::set_current_dir(&project.path)
            .with_context(|| format!("cannot enter {}", project.path))?;
    } else if git::repo_root(&env::current_dir()?).is_none() {
        bail!(
            "not inside a project; pass {} or cd into one",
            "--project <name>".bold()
        );
    }
    worktree::run(command)
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

    let github = GitHub::new(Some(github_auth::require_token()?));
    let login = github
        .current_user()
        .await?
        .context("GitHub authentication is invalid; run `p github login` or `gh auth login`")?;

    let (owner, repo) = name.split_once('/').unwrap_or((&login, name));
    validate_github_repo_parts(owner, repo)?;

    let target = project::github_checkout_path(owner, repo)?;
    if target.exists() {
        bail!("local target already exists: {}", target.display());
    }

    let org = (!owner.eq_ignore_ascii_case(&login)).then_some(owner);
    let created = github
        .create_repo(org, repo, description.as_deref())
        .await?;
    git::clone(&created.clone_url, &target, github.token())?;
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
            println!("{}", ui::style_scope(&g));
            for p in items {
                println!("  {}  {}", ui::style_name(&p.name), ui::style_path(&p.path));
            }
        }
    }
    Ok(())
}

async fn remove_cmd(db: &SqlitePool, path: Option<String>, force: bool) -> Result<()> {
    let target = resolve_remove_target(db, path).await?;
    let p = target.path();

    print_remove_target(&target, force);

    if !force {
        if ui::can_prompt_interactively() {
            let prompt = format!("Delete {} permanently?", p.display().to_string().dimmed());
            if !ui::confirm_yes_no(&prompt, false)? {
                println!("{} delete cancelled", "cancelled:".yellow().bold());
                return Ok(());
            }
        } else {
            ui::warn(&format!(
                "refusing to delete without {} in {}; re-run with {} to permanently delete the directory above",
                "--force".bold(),
                ui::terminal_interface().dimmed(),
                "--force".bold(),
            ));
            return Ok(());
        }
    }

    if let RemoveTarget::Project(project) = &target {
        db::begin_immediate_transaction(db).await?;
        let mark_result = async {
            if let Some(full_name) = project.github_full_name.as_deref() {
                db::mark_project_remote_only(db, &project.id, full_name).await
            } else {
                db::mark_project_deleted(db, &project.id).await
            }
        }
        .await;
        if let Err(error) = mark_result {
            let _ = db::rollback_transaction(db).await;
            return Err(error);
        }

        if let Err(error) = fs::remove_dir_all(p) {
            let _ = db::rollback_transaction(db).await;
            return Err(error).with_context(|| format!("failed to delete {}", p.display()));
        }

        db::commit_transaction(db).await?;
    } else {
        fs::remove_dir_all(p).with_context(|| format!("failed to delete {}", p.display()))?;
    }

    println!(
        "{} deleted {}",
        "removed:".green().bold(),
        p.display().to_string().dimmed()
    );
    Ok(())
}

async fn rename_cmd(db: &SqlitePool, project_name: &str, new_name: &str) -> Result<()> {
    validate_project_name(new_name)?;
    let project = db::find_project(db, project_name)
        .await?
        .with_context(|| format!("no project found for {project_name}"))?;

    let local_paths = if project.path.starts_with("github://") {
        None
    } else {
        let old_path = PathBuf::from(&project.path);
        if !old_path.exists() {
            bail!("project path does not exist: {}", old_path.display());
        }
        let parent = old_path
            .parent()
            .context("cannot rename a project without a parent directory")?;
        let new_path = parent.join(new_name);
        if new_path.exists() {
            bail!("rename target already exists: {}", new_path.display());
        }
        Some((old_path, new_path))
    };

    let new_github_full_name = if let Some(old_full_name) = project.github_full_name.as_deref() {
        let (owner, _) = old_full_name
            .split_once('/')
            .with_context(|| format!("invalid indexed GitHub repository name: {old_full_name}"))?;
        let full_name = GitHub::new(Some(github_auth::require_token()?))
            .rename_repo(old_full_name, new_name)
            .await?
            .full_name;
        if full_name != format!("{owner}/{new_name}") {
            bail!("GitHub returned an unexpected repository name after rename: {full_name}");
        }
        Some(full_name)
    } else {
        None
    };

    if let Some((old_path, new_path)) = &local_paths {
        fs::rename(old_path, new_path).with_context(|| {
            format!(
                "GitHub was renamed, but failed to rename {} to {}",
                old_path.display(),
                new_path.display()
            )
        })?;
    }

    let indexed_path = local_paths
        .as_ref()
        .map(|(_, new_path)| new_path.clone())
        .or_else(|| {
            new_github_full_name
                .as_ref()
                .map(|full_name| PathBuf::from(format!("github://{full_name}")))
        })
        .context("renamed project has neither a local path nor a GitHub repository")?;
    db::rename_project(
        db,
        &project.id,
        new_name,
        &indexed_path,
        new_github_full_name.as_deref(),
    )
    .await?;

    if let (Some((_, new_path)), Some(old_full_name), Some(new_full_name)) = (
        local_paths.as_ref(),
        project.github_full_name.as_deref(),
        new_github_full_name.as_deref(),
    ) {
        update_github_remote(new_path, old_full_name, new_full_name)?;
    }

    println!(
        "{} {} → {}",
        "renamed:".green().bold(),
        project.name.dimmed(),
        new_name.bold()
    );
    Ok(())
}

fn validate_project_name(name: &str) -> Result<()> {
    let mut components = Path::new(name).components();
    if !matches!(components.next(), Some(std::path::Component::Normal(_)))
        || components.next().is_some()
    {
        bail!("project name must be a single directory name");
    }
    if !name
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || ".-_".contains(character))
    {
        bail!(
            "project name may only contain ASCII letters, numbers, periods, hyphens, and underscores"
        );
    }
    Ok(())
}

fn update_github_remote(path: &Path, old_full_name: &str, new_full_name: &str) -> Result<()> {
    let old_url = git::output(path, &["remote", "get-url", "origin"])?;
    let new_url = old_url.replace(old_full_name, new_full_name);
    if new_url == old_url {
        bail!("renamed project, but could not recognize the GitHub origin URL: {old_url}");
    }
    git::run(path, &["remote", "set-url", "origin", &new_url])
}

enum RemoveTarget {
    Project(Project),
    Path(PathBuf),
}

impl RemoveTarget {
    fn path(&self) -> &Path {
        match self {
            RemoveTarget::Project(project) => Path::new(&project.path),
            RemoveTarget::Path(path) => path,
        }
    }
}

async fn resolve_remove_target(db: &SqlitePool, path: Option<String>) -> Result<RemoveTarget> {
    let Some(input) = path else {
        return Ok(RemoveTarget::Path(env::current_dir()?));
    };

    let candidate = PathBuf::from(&input);
    if candidate.exists() {
        return Ok(RemoveTarget::Path(candidate));
    }

    if let Some(project) = db::find_project(db, &input).await? {
        if project.path.starts_with("github://") {
            bail!(
                "found remote-only project {} at {}; there is no local directory to delete",
                project.name.bold(),
                project.path.dimmed()
            );
        }
        let project_path = Path::new(&project.path);
        if !project_path.exists() {
            bail!(
                "found project {} but its indexed path does not exist: {}",
                project.name.bold(),
                project.path.dimmed()
            );
        }
        return Ok(RemoveTarget::Project(project));
    }

    bail!(
        "no file, directory, or indexed project found for {}; run {} to refresh the project index",
        input.bold(),
        "p index".bold()
    )
}

fn print_remove_target(target: &RemoveTarget, force: bool) {
    let action = if force { "deleting" } else { "would delete" };
    println!("{}", format!("{action}:").yellow().bold());
    match target {
        RemoveTarget::Project(project) => {
            println!("  {} {}", "project:".bold(), ui::style_name(&project.name));
            println!("  {} {}", "path:".bold(), ui::style_path(&project.path));
            println!("  {} {}", "source:".bold(), project.source.dimmed());
            if let Some(scope) = project.scope.as_deref() {
                println!("  {} {}", "scope:".bold(), scope.dimmed());
            }
            if let Some(full_name) = project.github_full_name.as_deref() {
                println!("  {} {}", "github:".bold(), full_name.dimmed());
            }
        }
        RemoveTarget::Path(path) => {
            println!(
                "  {} {}",
                "path:".bold(),
                path.display().to_string().dimmed()
            );
        }
    }
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
                    println!("{} {}", ui::style_name(&n), "(hidden)".yellow());
                } else {
                    println!("{}", ui::style_name(&n));
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
        let Some(root) = git::repo_root(&path) else {
            if !quiet {
                ui::warn("not inside a git repository");
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
    let remote = match github_auth::load_token()? {
        Some(token) => {
            let repos = GitHub::new(Some(token)).user_repos().await?;
            for repo in &repos {
                db::upsert_remote_project(db, &repo.full_name).await?;
            }
            repos.len()
        }
        None => {
            if !quiet {
                ui::warn(&format!(
                    "remote indexing skipped; authenticate with {} first",
                    "p github login".bold()
                ));
            }
            0
        }
    };
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
