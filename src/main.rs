use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use directories::BaseDirs;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};
use uuid::Uuid;
use walkdir::WalkDir;

const DEFAULT_DEPTH: usize = 2;

#[derive(Debug, Clone, Serialize)]
struct Project {
    id: String,
    name: String,
    path: String,
    source: String,
    scope: Option<String>,
    github_full_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Config {
    search_roots: Option<Vec<String>>,
    search_depth: Option<usize>,
}

#[derive(Parser)]
#[command(
    name = "p",
    version,
    about = "Project manager — switch between projects quickly"
)]
struct Cli {
    name: Option<String>,
    #[arg(short = 'p', long)]
    path: bool,
    #[arg(short, long)]
    open: Option<String>,
    #[arg(short = 'a', long)]
    app: Option<String>,
    #[arg(short, long)]
    silent: bool,
    #[arg(long)]
    json: bool,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Resolve {
        name: String,
    },
    List {
        #[arg(long)]
        source: Option<String>,
        #[arg(long)]
        scope: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Remove {
        path: Option<String>,
        #[arg(short, long)]
        force: bool,
    },
    Dev,
    Config {
        #[command(subcommand)]
        command: ConfigCmd,
    },
    Org {
        #[command(subcommand)]
        command: OrgCmd,
    },
}
#[derive(Subcommand)]
enum ConfigCmd {
    List,
    Set { key: String, value: String },
    Delete { key: String },
}
#[derive(Subcommand)]
enum OrgCmd {
    List,
    Hide { name: String },
    Show { name: String },
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let db = db()?;
    migrate(&db)?;
    match cli.command {
        Some(Commands::Resolve { name }) => {
            if let Some(p) = find_project(&db, &name)? {
                print!("{}", p.path);
            } else {
                std::process::exit(1);
            }
        }
        Some(Commands::List {
            source,
            scope,
            json,
        }) => list(&db, source, scope, json)?,
        Some(Commands::Remove { path, force }) => remove_cmd(&db, path, force)?,
        Some(Commands::Dev) => dev_cmd()?,
        Some(Commands::Config { command }) => config_cmd(command)?,
        Some(Commands::Org { command }) => org_cmd(&db, command)?,
        None => select_cmd(&db, cli)?,
    }
    Ok(())
}

fn config_path() -> Result<PathBuf> {
    Ok(env::current_dir()?.join("config.json"))
}
fn load_config() -> Config {
    fs::read_to_string(config_path().unwrap())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Config {
            search_roots: None,
            search_depth: None,
        })
}
fn roots() -> Vec<PathBuf> {
    let c = load_config();
    c.search_roots
        .unwrap_or_else(|| {
            vec![
                BaseDirs::new()
                    .unwrap()
                    .home_dir()
                    .join("Documents")
                    .to_string_lossy()
                    .to_string(),
            ]
        })
        .into_iter()
        .map(PathBuf::from)
        .collect()
}
fn app_dir() -> Result<PathBuf> {
    let d = BaseDirs::new()
        .context("home dir unavailable")?
        .data_dir()
        .join("pm");
    fs::create_dir_all(&d)?;
    Ok(d)
}
fn db() -> Result<Connection> {
    Ok(Connection::open(app_dir()?.join("pm.db"))?)
}
fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS projects(id TEXT PRIMARY KEY,path TEXT UNIQUE NOT NULL,name TEXT NOT NULL,last_scanned INTEGER NOT NULL,last_modified INTEGER NOT NULL,is_git_repo INTEGER NOT NULL DEFAULT 1,file_count INTEGER,size_bytes INTEGER,created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL,deleted_at INTEGER,source TEXT DEFAULT 'local',github_full_name TEXT,scope TEXT NOT NULL DEFAULT 'personal',org_name TEXT NOT NULL DEFAULT '_local');CREATE TABLE IF NOT EXISTS project_history(id TEXT PRIMARY KEY,project_id TEXT NOT NULL,opened_at INTEGER NOT NULL,created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL,deleted_at INTEGER);CREATE TABLE IF NOT EXISTS orgs(name TEXT PRIMARY KEY,hidden INTEGER NOT NULL DEFAULT 0,created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL,deleted_at INTEGER);INSERT OR IGNORE INTO orgs(name,hidden,created_at,updated_at) VALUES('_local',0,unixepoch()*1000,unixepoch()*1000);")?;
    Ok(())
}
fn upsert_project(db: &Connection, path: &Path) -> Result<()> {
    let name = path.file_name().unwrap().to_string_lossy();
    let t = now();
    db.execute("INSERT INTO projects(id,path,name,last_scanned,last_modified,is_git_repo,created_at,updated_at,source,scope,org_name) VALUES(?1,?2,?3,?4,?4,1,?4,?4,'local','personal','_local') ON CONFLICT(path) DO UPDATE SET name=excluded.name,last_scanned=excluded.last_scanned,last_modified=excluded.last_modified,source='local',updated_at=excluded.updated_at,deleted_at=NULL", params![Uuid::new_v4().to_string(), path.to_string_lossy(), name.as_ref(), t])?;
    Ok(())
}
fn scan(db: &Connection) -> Result<()> {
    let depth = load_config().search_depth.unwrap_or(DEFAULT_DEPTH);
    for root in roots() {
        if !root.exists() {
            continue;
        }
        for e in WalkDir::new(root)
            .max_depth(depth + 1)
            .into_iter()
            .filter_map(Result::ok)
        {
            if e.file_type().is_dir() && e.path().join(".git").exists() {
                upsert_project(db, e.path())?;
            }
        }
    }
    Ok(())
}
fn projects(db: &Connection) -> Result<Vec<Project>> {
    scan(db)?;
    let mut st=db.prepare("SELECT id,name,path,source,scope,github_full_name FROM projects WHERE deleted_at IS NULL ORDER BY lower(name)")?;
    let rows = st.query_map([], |r| {
        Ok(Project {
            id: r.get(0)?,
            name: r.get(1)?,
            path: r.get(2)?,
            source: r.get(3)?,
            scope: r.get(4)?,
            github_full_name: r.get(5)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}
fn find_project(db: &Connection, name: &str) -> Result<Option<Project>> {
    let ps = projects(db)?;
    let low = name.to_lowercase();
    Ok(ps
        .iter()
        .find(|p| {
            p.name.to_lowercase() == low
                || p.github_full_name.as_deref().unwrap_or("").to_lowercase() == low
        })
        .cloned()
        .or_else(|| {
            ps.into_iter()
                .find(|p| p.name.to_lowercase().contains(&low))
        }))
}
fn select_cmd(db: &Connection, cli: Cli) -> Result<()> {
    let ps = projects(db)?;
    if cli.json {
        println!("{}", serde_json::to_string(&ps)?);
        return Ok(());
    }
    let p = if let Some(n) = cli.name {
        find_project(db, &n)?.context("no matching project")?
    } else {
        choose(ps)?
    };
    touch(db, &p)?;
    if cli.path || cli.silent {
        if cli.path {
            print!("{}", p.path);
        }
    } else if let Some(cmd) = cli.open {
        Command::new(cmd).arg(&p.path).status()?;
    } else if let Some(app) = cli.app {
        Command::new("open").args(["-a", &app, &p.path]).status()?;
    } else {
        println!("{}", p.path);
    }
    Ok(())
}
fn choose(ps: Vec<Project>) -> Result<Project> {
    if ps.is_empty() {
        bail!("no projects found")
    }
    for (i, p) in ps.iter().enumerate() {
        eprintln!("{:>3}  {}  {}", i + 1, p.name, p.path);
    }
    eprint!("select project number: ");
    let mut s = String::new();
    std::io::stdin().read_line(&mut s)?;
    let idx = s.trim().parse::<usize>().unwrap_or(1).saturating_sub(1);
    ps.get(idx).cloned().context("invalid selection")
}
fn touch(db: &Connection, p: &Project) -> Result<()> {
    let t = now();
    db.execute("INSERT INTO project_history(id,project_id,opened_at,created_at,updated_at) VALUES(?1,?2,?3,?3,?3)", params![Uuid::new_v4().to_string(), p.id, t])?;
    Ok(())
}
fn list(db: &Connection, source: Option<String>, scope: Option<String>, json: bool) -> Result<()> {
    let mut ps = projects(db)?;
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
fn remove_cmd(_db: &Connection, path: Option<String>, force: bool) -> Result<()> {
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
    let path = config_path()?;
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
fn org_cmd(db: &Connection, cmd: OrgCmd) -> Result<()> {
    match cmd {
        OrgCmd::List => {
            let mut st =
                db.prepare("SELECT name,hidden FROM orgs WHERE deleted_at IS NULL ORDER BY name")?;
            for r in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
                let (n, h) = r?;
                println!("{}{}", n, if h != 0 { " (hidden)" } else { "" });
            }
        }
        OrgCmd::Hide { name } => set_org_hidden(db, name, true)?,
        OrgCmd::Show { name } => set_org_hidden(db, name, false)?,
    }
    Ok(())
}
fn set_org_hidden(db: &Connection, name: String, hidden: bool) -> Result<()> {
    let t = now();
    db.execute("INSERT INTO orgs(name,hidden,created_at,updated_at) VALUES(?1,?2,?3,?3) ON CONFLICT(name) DO UPDATE SET hidden=excluded.hidden,updated_at=excluded.updated_at,deleted_at=NULL",params![name,hidden as i64,t])?;
    Ok(())
}
