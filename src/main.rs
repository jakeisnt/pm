mod cli;
mod config;
mod db;
mod project;

use anyhow::{Context, Result, bail};
use clap::Parser;
use cli::{Cli, Commands, ConfigCmd, OrgCmd};
use inquire::{
    Select,
    ui::{RenderConfig, StyleSheet, Styled},
};
use project::Project;
use sqlx::SqlitePool;
use std::{collections::BTreeMap, env, fs, path::PathBuf, process::Command};

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("error: {e:#}");
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
        db::find_project(db, &n)
            .await?
            .context("no matching project")?
    } else {
        choose(ps)?
    };
    db::touch(db, &p).await?;

    if cli.path || cli.silent {
        if cli.path {
            print!("{}", p.path);
        }
    } else if let Some(cmd) = cli.open {
        Command::new(cmd).arg(&p.path).status()?;
    } else if let Some(app) = cli.app {
        Command::new("open").args(["-a", &app, &p.path]).status()?;
    } else {
        spawn_shell_in(&p.path)?;
    }
    Ok(())
}

fn choose(ps: Vec<Project>) -> Result<Project> {
    if ps.is_empty() {
        bail!("no projects found")
    }

    let render_config = RenderConfig {
        highlighted_option_prefix: Styled::new("›").with_fg(inquire::ui::Color::LightCyan),
        selected_option: Some(StyleSheet::new().with_fg(inquire::ui::Color::LightGreen)),
        ..Default::default()
    };

    Select::new("Select project", ps)
        .with_help_message("Use ↑/↓ to move, type to filter, Enter to select")
        .with_page_size(20)
        .with_render_config(render_config)
        .prompt()
        .context("project selection cancelled")
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
            println!("{g}");
            for p in items {
                println!("  {}  {}", p.name, p.path);
            }
        }
    }
    Ok(())
}

fn remove_cmd(path: Option<String>, force: bool) -> Result<()> {
    let p = path.map(PathBuf::from).unwrap_or(env::current_dir()?);
    if !force {
        eprintln!("refusing to delete without --force: {}", p.display());
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
                println!("{}{}", n, if h != 0 { " (hidden)" } else { "" });
            }
        }
        OrgCmd::Hide { name } => db::set_org_hidden(db, name, true).await?,
        OrgCmd::Show { name } => db::set_org_hidden(db, name, false).await?,
    }
    Ok(())
}
