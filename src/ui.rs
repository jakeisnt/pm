//! Terminal interaction shared by every command: pickers, prompts, styling,
//! status lines and handing the terminal to a shell or agent.

use anyhow::{Context, Result};
use colored::{Color, Colorize};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    terminal,
};
use regex::Regex;
use skim::{prelude::*, tui::BorderType};
use std::{
    env,
    io::{self, Cursor, IsTerminal, Write},
    path::{Path, PathBuf},
    process::Command,
};

pub fn style_scope(scope: &str) -> colored::ColoredString {
    scope.bold().color(Color::BrightCyan)
}

/// Fuzzy-pick one tab-separated line. The first two columns are shown and
/// searched; the rest (e.g. an id) only come back in the returned line.
pub fn fuzzy_select(
    prompt: &str,
    input: &str,
    initial_query: Option<&str>,
) -> Result<Option<String>> {
    let mut options_builder = SkimOptionsBuilder::default();
    options_builder
        .prompt(format!("{} ", prompt.bright_cyan().bold()))
        .height("80%")
        .reverse(true)
        .border(BorderType::Rounded)
        .with_nth(vec!["1".to_string(), "2".to_string()])
        .delimiter(Regex::new("\t").context("failed to configure selector delimiter")?);
    if let Some(query) = initial_query {
        options_builder.query(query.to_string());
    }
    let options = options_builder
        .build()
        .context("failed to configure selector")?;
    let item_reader = SkimItemReader::new(SkimItemReaderOption::default().ansi(true));
    let items = item_reader.of_bufread(Cursor::new(input.to_string()));
    let output = Skim::run_with(options, Some(items))
        .map_err(|err| anyhow::anyhow!("failed to run selector: {err}"))?;
    if output.is_abort {
        return Ok(None);
    }
    Ok(output
        .selected_items
        .first()
        .map(|item| item.output().to_string()))
}

pub fn style_name(name: &str) -> colored::ColoredString {
    name.bold().color(Color::BrightGreen)
}

pub fn style_path(path: &str) -> colored::ColoredString {
    path.dimmed()
}

pub fn spawn_agent_in(path: &str) -> Result<()> {
    Command::new("pi")
        .current_dir(path)
        .status()
        .context("failed to launch pi")?;
    Ok(())
}

pub fn spawn_shell_in(path: &str) -> Result<()> {
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

pub fn confirm_yes_no(prompt: &str, default: bool) -> Result<bool> {
    let suffix = if default { " [Y/n] " } else { " [y/N] " };
    print!("{prompt}{suffix}");
    io::stdout().flush()?;

    terminal::enable_raw_mode().context("failed to enable raw terminal mode")?;
    let result = loop {
        let event = match event::read().context("failed to read terminal input") {
            Ok(event) => event,
            Err(error) => break Err(error),
        };
        match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => break Ok(true),
                KeyCode::Char('n') | KeyCode::Char('N') => break Ok(false),
                KeyCode::Enter => break Ok(default),
                KeyCode::Esc => break Ok(false),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    break Err(anyhow::anyhow!("confirmation interrupted"));
                }
                _ => {}
            },
            _ => {}
        }
    };
    let disable_result =
        terminal::disable_raw_mode().context("failed to disable raw terminal mode");

    match (&result, default) {
        (Ok(true), _) => println!("y"),
        (Ok(false), _) => println!("n"),
        (Err(_), _) => println!(),
    }

    disable_result?;
    result
}

pub fn can_prompt_interactively() -> bool {
    if env::var_os("CI").is_some() || env::var_os("GITHUB_ACTIONS").is_some() {
        return false;
    }

    io::stdin().is_terminal() && io::stdout().is_terminal()
}

pub fn terminal_interface() -> String {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return "a non-interactive terminal".to_string();
    }

    env::var("TERM_PROGRAM")
        .map(|program| format!("{program} terminal"))
        .or_else(|_| env::var("TERM").map(|term| format!("{term} terminal")))
        .unwrap_or_else(|_| "this terminal".to_string())
}

// Status lines (worktrunk-style symbols), all on stderr so stdout stays scriptable.

pub fn success(msg: &str) {
    eprintln!("{} {msg}", "✓".green().bold());
}

pub fn progress(msg: &str) {
    eprintln!("{} {msg}", "◎".cyan().bold());
}

pub fn info(msg: &str) {
    eprintln!("{} {msg}", "○".dimmed());
}

pub fn warn(msg: &str) {
    eprintln!("{} {msg}", "▲".yellow().bold());
}

/// Display a path with the home directory shortened to `~`.
pub fn tildify(path: &Path) -> String {
    if let Some(dirs) = directories::BaseDirs::new()
        && let Ok(rest) = path.strip_prefix(dirs.home_dir())
    {
        return format!("~/{}", rest.display());
    }
    path.display().to_string()
}
