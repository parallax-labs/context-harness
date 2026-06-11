//! Secret redaction for connector and config values surfaced to operators
//! (the `ctx sources` / `GET /tools/sources` listing, the `workspaces`
//! discovery tool, and any future config-introspection surface).
//!
//! Policy is **deny-by-default**: when emitting an arbitrary connector config
//! map — for example a script connector's `extra` table, whose `${VAR}` values
//! are env-expanded and may be credentials — only an allowlist of known-safe
//! structural keys is shown verbatim; URL-bearing keys have their embedded
//! credentials stripped; every other value is replaced with [`REDACTED`].
//!
//! This satisfies SPEC-0014 R48/R49: redact credentials, tokens, and
//! env-expanded values, not only fields whose names literally contain
//! `secret`/`token`.

use toml::Value;

/// Marker substituted for any redacted value.
pub const REDACTED: &str = "[REDACTED]";

/// Structural, non-secret connector keys that are safe to show verbatim.
const SAFE_KEYS: &[&str] = &[
    "path",
    "root",
    "branch",
    "region",
    "bucket",
    "prefix",
    "include_globs",
    "exclude_globs",
    "shallow",
    "follow_symlinks",
    "max_extract_bytes",
    "timeout",
    "timeout_secs",
    "enabled",
    "name",
    "dims",
    "batch_size",
    "max_retries",
    "provider",
    "model",
    "metric",
    "backend",
    "index",
    "fallback",
];

/// Keys whose values are URLs: shown with embedded credentials stripped.
const URL_KEYS: &[&str] = &["url", "endpoint_url", "endpoint", "host"];

/// Strip credentials (userinfo) from a URL.
///
/// `https://user:token@host/path` becomes `https://[REDACTED]@host/path`. The
/// scheme, host, port, and path are preserved. Values without a `scheme://`
/// authority (scp-like `git@host:path`, bare local paths) are returned
/// unchanged — they carry no `user:secret@` userinfo to leak.
pub fn redact_url(value: &str) -> String {
    let Some(scheme_end) = value.find("://") else {
        return value.to_string();
    };
    let auth_start = scheme_end + 3;
    let rest = &value[auth_start..];
    let auth_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..auth_end];
    match authority.rfind('@') {
        Some(at) => {
            let host = &authority[at + 1..];
            format!(
                "{}{}@{}{}",
                &value[..auth_start],
                REDACTED,
                host,
                &rest[auth_end..]
            )
        }
        None => value.to_string(),
    }
}

/// Redact an arbitrary connector-config table, deny-by-default.
///
/// Returns a new table safe to serialize into operator-facing output. Nested
/// tables and arrays are redacted recursively. Unknown keys (anything not in
/// the structural allowlist or a recognized URL key) have their values
/// replaced with [`REDACTED`].
pub fn redact_connector_map(table: &toml::value::Table) -> toml::value::Table {
    table
        .iter()
        .map(|(k, v)| (k.clone(), redact_value(k, v)))
        .collect()
}

fn redact_value(key: &str, value: &Value) -> Value {
    let lk = key.to_ascii_lowercase();
    match value {
        Value::Table(t) => Value::Table(redact_connector_map(t)),
        Value::Array(a) => Value::Array(a.iter().map(|v| redact_value(key, v)).collect()),
        Value::String(s) if URL_KEYS.contains(&lk.as_str()) => Value::String(redact_url(s)),
        _ if SAFE_KEYS.contains(&lk.as_str()) => value.clone(),
        _ => Value::String(REDACTED.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_userinfo_from_https_url() {
        assert_eq!(
            redact_url("https://user:ghp_secrettoken@github.com/acme/platform.git"),
            "https://[REDACTED]@github.com/acme/platform.git"
        );
    }

    #[test]
    fn redacts_token_only_userinfo() {
        assert_eq!(
            redact_url("https://x-access-token:abc123@github.com/o/r.git"),
            "https://[REDACTED]@github.com/o/r.git"
        );
    }

    #[test]
    fn leaves_clean_url_unchanged() {
        let url = "https://github.com/acme/platform.git";
        assert_eq!(redact_url(url), url);
    }

    #[test]
    fn leaves_scp_like_and_bare_paths_unchanged() {
        // scp-like git remote: `git@` is a username, not a secret; no `://`.
        assert_eq!(
            redact_url("git@github.com:acme/platform.git"),
            "git@github.com:acme/platform.git"
        );
        assert_eq!(
            redact_url("/srv/repos/platform.git"),
            "/srv/repos/platform.git"
        );
    }

    #[test]
    fn preserves_query_and_port() {
        assert_eq!(
            redact_url("http://user:pw@localhost:9000/bucket?x=1"),
            "http://[REDACTED]@localhost:9000/bucket?x=1"
        );
    }

    #[test]
    fn connector_map_is_deny_by_default() {
        let mut t = toml::value::Table::new();
        t.insert(
            "base_url".into(),
            Value::String("https://svc.example.com".into()),
        );
        t.insert(
            "url".into(),
            Value::String("https://u:p@git.example.com/x".into()),
        );
        t.insert(
            "api_token".into(),
            Value::String("${JIRA_API_TOKEN}".into()),
        );
        t.insert("project".into(), Value::String("ENG".into()));
        t.insert("path".into(), Value::String("/scripts/jira.lua".into()));

        let r = redact_connector_map(&t);
        // URL key: creds stripped, host kept.
        assert_eq!(
            r["url"].as_str().unwrap(),
            "https://[REDACTED]@git.example.com/x"
        );
        // Safe structural key: kept verbatim.
        assert_eq!(r["path"].as_str().unwrap(), "/scripts/jira.lua");
        // Unknown keys (incl. non-token-named secrets): redacted.
        assert_eq!(r["api_token"].as_str().unwrap(), REDACTED);
        assert_eq!(r["project"].as_str().unwrap(), REDACTED);
        // base_url is not in URL_KEYS or SAFE_KEYS -> redacted (deny-by-default).
        assert_eq!(r["base_url"].as_str().unwrap(), REDACTED);
    }
}
