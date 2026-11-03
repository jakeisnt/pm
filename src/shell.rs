//! Shell startup hooks: index repos after `git init/clone/worktree add`, and let
//! `p worktree` commands change the shell's directory.

use crate::cli::Shell;
use anyhow::{Context, Result};
use colored::Colorize;
use std::{
    env, fs,
    path::{Path, PathBuf},
};

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
# p: let `p worktree` commands change the shell's directory.
p() {
  local __p_cd_file __p_status
  __p_cd_file="$(mktemp "${TMPDIR:-/tmp}/p-cd.XXXXXX")" || { command p "$@"; return; }
  PM_CD_FILE="$__p_cd_file" command p "$@"
  __p_status=$?
  if [ -s "$__p_cd_file" ]; then
    cd "$(cat "$__p_cd_file")" || __p_status=$?
  fi
  rm -f "$__p_cd_file"
  return $__p_status
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
# p: let `p worktree` commands change the shell's directory.
p() {
  local __p_cd_file __p_status
  __p_cd_file="$(mktemp "${TMPDIR:-/tmp}/p-cd.XXXXXX")" || { command p "$@"; return; }
  PM_CD_FILE="$__p_cd_file" command p "$@"
  __p_status=$?
  if [ -s "$__p_cd_file" ]; then
    cd "$(cat "$__p_cd_file")" || __p_status=$?
  fi
  rm -f "$__p_cd_file"
  return $__p_status
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
# p: let `p worktree` commands change the shell's directory.
function p
  set -l cd_file (mktemp); or begin; command p $argv; return; end
  PM_CD_FILE=$cd_file command p $argv
  set -l p_status $status
  if test -s $cd_file
    cd (cat $cd_file); or set p_status $status
  end
  rm -f $cd_file
  return $p_status
end
"#;

fn hook_text(shell: Shell) -> &'static str {
    match shell {
        Shell::Zsh => ZSH_HOOK,
        Shell::Bash => BASH_HOOK,
        Shell::Fish => FISH_HOOK,
    }
}

pub fn print_hook(shell: Shell) {
    print!("{}", hook_text(shell));
}

pub fn install_hook(shell: Option<Shell>) -> Result<()> {
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
