//! The subset of worktrunk's template syntax `p` supports:
//! `{{ var }}` and `{{ var | filter | ... }}`. `{% ... %}` blocks are rejected.

use anyhow::{Context, Result, bail};
use regex::Regex;
use std::{collections::BTreeMap, path::Path, sync::LazyLock};

pub type Vars = BTreeMap<String, String>;

static EXPR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{\{\s*([^}]*?)\s*\}\}").expect("valid regex"));

pub fn render(template: &str, vars: &Vars) -> Result<String> {
    if template.contains("{%") {
        bail!("{{% ... %}} blocks are not supported by p; simplify the template: {template}");
    }
    let mut out = String::new();
    let mut last = 0;
    for cap in EXPR.captures_iter(template) {
        let whole = cap.get(0).expect("match");
        out.push_str(&template[last..whole.start()]);
        let mut parts = cap[1].split('|').map(str::trim);
        let var = parts.next().unwrap_or_default();
        let mut value = vars
            .get(var)
            .cloned()
            .with_context(|| format!("undefined template variable: {var}"))?;
        for filter in parts {
            value = apply(filter, &value)?;
        }
        out.push_str(&value);
        last = whole.end();
    }
    out.push_str(&template[last..]);
    Ok(out)
}

fn apply(filter: &str, value: &str) -> Result<String> {
    let path = Path::new(value);
    Ok(match filter {
        "sanitize" => sanitize(value),
        "basename" => path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        "dirname" => path
            .parent()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        "hash_port" => (10_000 + fnv1a(value.as_bytes()) % 10_000).to_string(),
        "lower" => value.to_lowercase(),
        "upper" => value.to_uppercase(),
        other => bail!("unsupported template filter: {other}"),
    })
}

/// Filesystem-safe branch name: `/` and `\` become `-`.
pub fn sanitize(s: &str) -> String {
    s.replace(['/', '\\'], "-")
}

/// Stable 64-bit FNV-1a hash.
pub fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars() -> Vars {
        Vars::from([
            ("repo_path".to_string(), "/code/pm".to_string()),
            ("branch".to_string(), "feat/auth".to_string()),
        ])
    }

    #[test]
    fn renders_variables_and_filters() {
        let v = vars();
        assert_eq!(render("{{branch|sanitize}}", &v).unwrap(), "feat-auth");
        assert_eq!(render("{{repo_path|basename}}", &v).unwrap(), "pm");
        assert_eq!(render("{{ repo_path | dirname }}", &v).unwrap(), "/code");
        let port: u32 = render("{{ branch | hash_port }}", &v)
            .unwrap()
            .parse()
            .unwrap();
        assert!((10_000..20_000).contains(&port));
    }

    #[test]
    fn rejects_unknowns_and_blocks() {
        let v = vars();
        assert!(render("{{ nope }}", &v).is_err());
        assert!(render("{{ branch | wat }}", &v).is_err());
        assert!(render("{% if branch %}x{% endif %}", &v).is_err());
    }
}
