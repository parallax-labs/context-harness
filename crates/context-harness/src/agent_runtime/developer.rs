//! Workspace developer tools. Path checks protect ordinary filesystem access;
//! process execution is an explicit, unsandboxed capability.
use crate::{
    agent_resource::Capability,
    traits::{Tool, ToolContext, ToolRegistry},
};
use anyhow::{bail, ensure, Context, Result};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    path::{Component, Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{io::AsyncReadExt, process::Command};

const FILE_LIMIT: usize = 1024 * 1024;
const OUTPUT_LIMIT: usize = 128 * 1024;
const NAMES: [&str; 6] = [
    "workspace.read",
    "workspace.search",
    "git.status",
    "git.diff",
    "workspace.patch",
    "process.exec",
];

pub(super) fn capability(name: &str) -> Option<Capability> {
    match name {
        "workspace.read" | "workspace.search" | "git.status" | "git.diff" => {
            Some(Capability::ReadOnly)
        }
        "workspace.patch" => Some(Capability::WorkspaceWrite),
        "process.exec" => Some(Capability::ProcessExecute),
        _ => None,
    }
}
pub(super) fn register(registry: &mut ToolRegistry, root: &Path) -> Result<()> {
    let root = root.canonicalize()?;
    for name in NAMES {
        registry.register(Box::new(DeveloperTool {
            name,
            root: root.clone(),
        }));
    }
    Ok(())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadArgs {
    path: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchArgs {
    path: String,
    query: String,
    #[serde(default = "default_limit")]
    limit: usize,
}
fn default_limit() -> usize {
    100
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PatchArgs {
    path: String,
    old_text: String,
    new_text: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProcessArgs {
    argv: Vec<String>,
    #[serde(default = "default_cwd")]
    cwd: String,
    #[serde(default = "default_timeout")]
    timeout_seconds: u64,
}
fn default_cwd() -> String {
    ".".into()
}
fn default_timeout() -> u64 {
    30
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyArgs {}
fn relative(path: &str) -> Result<()> {
    ensure!(
        !path.is_empty() && path.len() <= 4096,
        "invalid workspace path"
    );
    ensure!(
        !Path::new(path).is_absolute() && !path.contains('\0'),
        "workspace path must be relative"
    );
    for component in Path::new(path).components() {
        ensure!(
            !matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            ),
            "path traversal is forbidden"
        );
        ensure!(
            !component
                .as_os_str()
                .to_string_lossy()
                .eq_ignore_ascii_case(".git"),
            "git metadata is not accessible"
        );
    }
    Ok(())
}
fn confined(root: &Path, path: &str) -> Result<PathBuf> {
    relative(path)?;
    let resolved = root
        .join(path)
        .canonicalize()
        .context("workspace path does not exist")?;
    let relative = resolved
        .strip_prefix(root)
        .context("workspace path escapes root")?;
    ensure!(
        !relative
            .components()
            .any(|c| c.as_os_str().to_string_lossy().eq_ignore_ascii_case(".git")),
        "git metadata is not accessible"
    );
    Ok(resolved)
}
pub(super) fn validate(root: &Path, name: &str, args: &Value) -> Result<()> {
    match name {
        "workspace.read" => relative(&serde_json::from_value::<ReadArgs>(args.clone())?.path)?,
        "workspace.search" => {
            let a: SearchArgs = serde_json::from_value(args.clone())?;
            relative(&a.path)?;
            ensure!(
                !a.query.is_empty() && a.query.len() <= 8192 && (1..=100).contains(&a.limit),
                "invalid search arguments"
            );
        }
        "workspace.patch" => {
            let a: PatchArgs = serde_json::from_value(args.clone())?;
            writable(root, &a.path)?;
            ensure!(
                !a.old_text.is_empty()
                    && a.old_text.len() <= FILE_LIMIT
                    && a.new_text.len() <= FILE_LIMIT,
                "invalid replacement"
            );
        }
        "process.exec" => {
            let a: ProcessArgs = serde_json::from_value(args.clone())?;
            relative(&a.cwd)?;
            ensure!(
                !a.argv.is_empty()
                    && a.argv.len() <= 128
                    && !a.argv[0].is_empty()
                    && a.argv.iter().all(|v| !v.contains('\0'))
                    && a.argv.iter().map(String::len).sum::<usize>() <= 65536
                    && (1..=60).contains(&a.timeout_seconds),
                "invalid process arguments"
            );
        }
        "git.status" | "git.diff" => {
            let _: EmptyArgs = serde_json::from_value(args.clone())?;
        }
        _ => bail!("unsupported developer tool"),
    }
    if let Some(path) = args
        .get("path")
        .or_else(|| args.get("cwd"))
        .and_then(Value::as_str)
    {
        confined(root, path)?;
    }
    Ok(())
}
fn writable(root: &Path, path: &str) -> Result<PathBuf> {
    let path = confined(root, path)?;
    let parts: Vec<_> = path.strip_prefix(root)?.components().collect();
    ensure!(
        !(parts.len() >= 2
            && parts[0]
                .as_os_str()
                .to_string_lossy()
                .eq_ignore_ascii_case(".ctx")
            && parts[1]
                .as_os_str()
                .to_string_lossy()
                .eq_ignore_ascii_case("runs")),
        "runtime history and artifacts cannot be patched"
    );
    Ok(path)
}
fn read_file(path: &Path) -> Result<String> {
    use std::io::Read;
    ensure!(std::fs::metadata(path)?.is_file(), "expected regular file");
    let file = std::fs::File::open(path)?;
    ensure!(file.metadata()?.is_file(), "expected regular file");
    let mut bytes = Vec::new();
    file.take((FILE_LIMIT + 1) as u64).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= FILE_LIMIT, "file exceeds 1 MiB limit");
    String::from_utf8(bytes).context("expected UTF-8 text")
}
async fn bounded_read(reader: impl tokio::io::AsyncRead + Unpin) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    reader
        .take((OUTPUT_LIMIT + 1) as u64)
        .read_to_end(&mut output)
        .await?;
    ensure!(output.len() <= OUTPUT_LIMIT, "process output exceeds limit");
    Ok(output)
}
async fn process(mut cmd: Command, seconds: u64) -> Result<Value> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = cmd.spawn().context("could not start process")?;
    let stdout = child.stdout.take().context("missing stdout")?;
    let stderr = child.stderr.take().context("missing stderr")?;
    let result = tokio::time::timeout(Duration::from_secs(seconds), async {
        tokio::try_join!(
            async { Ok::<_, anyhow::Error>(child.wait().await?) },
            bounded_read(stdout),
            bounded_read(stderr)
        )
    })
    .await;
    match result {
        Ok(Ok((status, stdout, stderr))) => Ok(
            json!({"exit_code":status.code(), "success":status.success(), "stdout":String::from_utf8_lossy(&stdout), "stderr":String::from_utf8_lossy(&stderr)}),
        ),
        other => {
            let _ = child.kill().await;
            match other {
                Err(_) => bail!("process timed out"),
                Ok(Err(error)) => Err(error),
                _ => unreachable!(),
            }
        }
    }
}
fn git_command(root: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .current_dir(root);
    cmd.arg("--work-tree")
        .arg(root)
        .arg("--git-dir")
        .arg(root.join(".git"));
    cmd.args([
        "--no-pager",
        "--no-optional-locks",
        "-c",
        "core.fsmonitor=false",
        "-c",
        "core.untrackedCache=false",
        "-c",
        "core.hooksPath=/dev/null",
        "-c",
        "core.attributesFile=/dev/null",
        "-c",
        "diff.external=",
        "-c",
        "core.pager=cat",
    ]);
    cmd
}
struct DeveloperTool {
    name: &'static str,
    root: PathBuf,
}
#[async_trait]
impl Tool for DeveloperTool {
    fn name(&self) -> &str {
        self.name
    }
    fn description(&self) -> &str {
        match self.name {
            "workspace.read" => "Read a UTF-8 workspace file (maximum 1 MiB)",
            "workspace.search" => {
                "Find literal text in workspace files; skips symlinks and git metadata"
            }
            "git.status" => "Read workspace Git status without optional index writes or hooks",
            "git.diff" => {
                "Read unstaged workspace Git diff without external diff or text conversion"
            }
            "workspace.patch" => {
                "Replace one unique old_text with new_text in an existing workspace UTF-8 file"
            }
            _ => {
                "Execute explicit argv in a workspace directory; UNSANDBOXED with host permissions"
            }
        }
    }
    fn capabilities(&self) -> Option<Vec<Capability>> {
        capability(self.name).map(|c| vec![c])
    }
    fn is_builtin(&self) -> bool {
        true
    }
    fn parameters_schema(&self) -> Value {
        let (properties, required) = match self.name {
            "workspace.read" => (json!({"path":{"type":"string"}}), json!(["path"])),
            "workspace.search" => (
                json!({"path":{"type":"string"},"query":{"type":"string","minLength":1},"limit":{"type":"integer","minimum":1,"maximum":100,"default":100}}),
                json!(["path", "query"]),
            ),
            "workspace.patch" => (
                json!({"path":{"type":"string"},"old_text":{"type":"string","minLength":1},"new_text":{"type":"string"}}),
                json!(["path", "old_text", "new_text"]),
            ),
            "process.exec" => (
                json!({"argv":{"type":"array","minItems":1,"maxItems":128,"items":{"type":"string"}},"cwd":{"type":"string","default":"."},"timeout_seconds":{"type":"integer","minimum":1,"maximum":60,"default":30}}),
                json!(["argv"]),
            ),
            _ => (json!({}), json!([])),
        };
        json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
    }
    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<Value> {
        validate(&self.root, self.name, &args)?;
        self.execute_validated(args).await
    }
}
impl DeveloperTool {
    async fn execute_validated(&self, args: Value) -> Result<Value> {
        match self.name {
            "workspace.read" => {
                let a: ReadArgs = serde_json::from_value(args)?;
                let path = confined(&self.root, &a.path)?;
                Ok(json!({"path":a.path,"text":read_file(&path)?}))
            }
            "workspace.search" => {
                let a: SearchArgs = serde_json::from_value(args)?;
                let path = confined(&self.root, &a.path)?;
                let mut matches = Vec::new();
                let mut scanned = 0usize;
                let mut bytes_scanned = 0usize;
                let mut truncated = false;
                let entries = walkdir::WalkDir::new(path)
                    .follow_links(false)
                    .into_iter()
                    .filter_entry(|e| {
                        !e.file_name().to_string_lossy().eq_ignore_ascii_case(".git")
                    });
                for (visited, entry) in entries.enumerate() {
                    if visited >= 10000
                        || scanned >= 1000
                        || bytes_scanned >= 16 * FILE_LIMIT
                        || matches.len() >= a.limit
                    {
                        truncated = true;
                        break;
                    }
                    let entry = entry?;
                    if !entry.file_type().is_file() {
                        continue;
                    }
                    let relative = entry
                        .path()
                        .strip_prefix(&self.root)?
                        .to_str()
                        .context("non-UTF8 file path")?;
                    let checked = confined(&self.root, relative)?;
                    tokio::task::yield_now().await;
                    scanned += 1;
                    let Ok(text) = read_file(&checked) else {
                        continue;
                    };
                    bytes_scanned += text.len();
                    for (line, content) in text.lines().enumerate() {
                        if content.contains(&a.query) {
                            // Bound each snippet by characters so UTF-8 is never split.
                            matches.push(json!({"path":relative,"line":line + 1,"text":content.chars().take(1000).collect::<String>()}));
                            if matches.len() >= a.limit {
                                truncated = true;
                                break;
                            }
                        }
                    }
                }
                Ok(json!({"matches":matches,"files_scanned":scanned,"truncated":truncated}))
            }
            "workspace.patch" => {
                let a: PatchArgs = serde_json::from_value(args)?;
                let path = writable(&self.root, &a.path)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    ensure!(
                        std::fs::metadata(&path)?.nlink() == 1,
                        "cannot patch hard-linked files"
                    );
                }
                let old = read_file(&path)?;
                let first = old
                    .find(&a.old_text)
                    .context("old_text must match exactly once")?;
                let next = first
                    + a.old_text
                        .chars()
                        .next()
                        .context("empty old_text")?
                        .len_utf8();
                ensure!(
                    !old[next..].contains(&a.old_text),
                    "old_text must match exactly once"
                );
                let new = old.replacen(&a.old_text, &a.new_text, 1);
                ensure!(new.len() <= FILE_LIMIT, "replacement exceeds file limit");
                // Synchronous replacement completes without an async cancellation point.
                std::fs::write(&path, new.as_bytes())?;
                Ok(json!({"path":a.path,"bytes_written":new.len()}))
            }
            "process.exec" => {
                let a: ProcessArgs = serde_json::from_value(args)?;
                let cwd = confined(&self.root, &a.cwd)?;
                ensure!(cwd.is_dir(), "cwd must be a directory");
                let mut cmd = Command::new(&a.argv[0]);
                cmd.args(&a.argv[1..]).current_dir(cwd);
                process(cmd, a.timeout_seconds).await
            }
            _ => {
                ensure!(
                    self.root.join(".git").exists(),
                    "Git tools require a workspace repository"
                );
                // Git may run clean/process filters even during status/diff.
                // Enumerate effective filter keys (including local includes) and
                // override them, along with fsmonitor and external diff helpers.
                let mut config = git_command(&self.root);
                config.args([
                    "config",
                    "--includes",
                    "--name-only",
                    "--get-regexp",
                    "^filter\\..*\\.(clean|process|required)$",
                ]);
                let filters = process(config, 10).await?;
                ensure!(
                    matches!(filters["exit_code"].as_i64(), Some(0 | 1)),
                    "could not inspect Git filters"
                );
                let mut cmd = git_command(&self.root);
                for key in filters["stdout"].as_str().unwrap_or("").lines() {
                    ensure!(
                        key.starts_with("filter.") && !key.contains('=') && !key.contains('\0'),
                        "invalid Git filter key"
                    );
                    let value = if key.ends_with(".required") {
                        "false"
                    } else {
                        ""
                    };
                    cmd.arg("-c").arg(format!("{key}={value}"));
                }
                if self.name == "git.status" {
                    cmd.args([
                        "status",
                        "--porcelain=v1",
                        "--untracked-files=normal",
                        "--ignore-submodules=all",
                    ]);
                } else {
                    cmd.args([
                        "diff",
                        "--no-ext-diff",
                        "--no-textconv",
                        "--ignore-submodules=all",
                        "--no-color",
                        "--",
                    ]);
                }
                process(cmd, 30).await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tool(name: &'static str, root: &Path) -> DeveloperTool {
        DeveloperTool {
            name,
            root: root.canonicalize().unwrap(),
        }
    }
    #[test]
    fn strict_arguments() {
        for (name, args) in [
            ("workspace.read", json!({"path":"../secret"})),
            ("workspace.read", json!({"path":"/etc/passwd"})),
            ("workspace.read", json!({"path":".git/config"})),
            (
                "workspace.search",
                json!({"path":".","query":"x","limit":0}),
            ),
            ("process.exec", json!({"argv":[]})),
            ("process.exec", json!({"argv":["echo"],"shell":true})),
            ("git.diff", json!({"path":"x"})),
        ] {
            assert!(validate(Path::new("."), name, &args).is_err());
        }
    }
    #[tokio::test]
    async fn reads_searches_and_patches_unique_text() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("sample"), "hello world\nnext\n").unwrap();
        let result = tool("workspace.read", root.path())
            .execute_validated(json!({"path":"sample"}))
            .await
            .unwrap();
        assert_eq!(result["text"], "hello world\nnext\n");
        let result = tool("workspace.search", root.path())
            .execute_validated(json!({"path":".","query":"world"}))
            .await
            .unwrap();
        assert_eq!(result["matches"][0]["line"], 1);
        tool("workspace.patch", root.path())
            .execute_validated(json!({"path":"sample","old_text":"world","new_text":"workspace"}))
            .await
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(root.path().join("sample")).unwrap(),
            "hello workspace\nnext\n"
        );
        assert!(tool("workspace.patch", root.path())
            .execute_validated(json!({"path":"sample","old_text":"missing","new_text":"x"}))
            .await
            .is_err());
        std::fs::write(root.path().join("overlap"), "aaa").unwrap();
        assert!(tool("workspace.patch", root.path())
            .execute_validated(json!({"path":"overlap","old_text":"aa","new_text":"x"}))
            .await
            .is_err());
        std::fs::create_dir_all(root.path().join(".ctx/runs")).unwrap();
        std::fs::write(root.path().join(".ctx/runs/history"), "old").unwrap();
        assert!(validate(
            root.path(),
            "workspace.patch",
            &json!({"path":".ctx/runs/history", "old_text":"old", "new_text":"new"})
        )
        .is_err());
        std::fs::write(root.path().join("big"), vec![b'a'; FILE_LIMIT + 1]).unwrap();
        assert!(tool("workspace.read", root.path())
            .execute_validated(json!({"path":"big"}))
            .await
            .is_err());
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn refuses_symlink_escapes_and_hardlink_writes() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let secret = outside.path().join("secret");
        std::fs::write(&secret, "secret").unwrap();
        symlink(&secret, root.path().join("escape")).unwrap();
        assert!(tool("workspace.read", root.path())
            .execute_validated(json!({"path":"escape"}))
            .await
            .is_err());
        std::fs::hard_link(&secret, root.path().join("hardlink")).unwrap();
        assert!(tool("workspace.patch", root.path())
            .execute_validated(json!({"path":"hardlink","old_text":"secret","new_text":"changed"}))
            .await
            .is_err());
        assert_eq!(std::fs::read_to_string(secret).unwrap(), "secret");
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn process_reports_exit_and_bounds_time_and_output() {
        let root = tempfile::tempdir().unwrap();
        let exec = tool("process.exec", root.path());
        let result = exec
            .execute_validated(
                json!({"argv":["/bin/sh","-c","printf output; printf error >&2; exit 7"]}),
            )
            .await
            .unwrap();
        assert_eq!(result["stdout"], "output");
        assert_eq!(result["stderr"], "error");
        assert_eq!(result["exit_code"], 7);
        assert!(exec
            .execute_validated(json!({"argv":["/bin/sleep","3"],"timeout_seconds":1}))
            .await
            .unwrap_err()
            .to_string()
            .contains("timed out"));
        assert!(exec
            .execute_validated(json!({"argv":["/usr/bin/yes"]}))
            .await
            .unwrap_err()
            .to_string()
            .contains("output exceeds"));
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn git_disables_clean_filters_and_handles_worktrees() {
        let root = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            let result = std::process::Command::new("git")
                .current_dir(root.path())
                .args(args)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "test@example.com"]);
        git(&["config", "user.name", "Test"]);
        std::fs::write(root.path().join("sample"), "original").unwrap();
        git(&["add", "sample"]);
        git(&["-c", "commit.gpgsign=false", "commit", "-qm", "initial"]);
        std::fs::write(
            root.path().join(".gitattributes"),
            "sample filter=unsafe diff=unsafe\n",
        )
        .unwrap();
        git(&[
            "config",
            "filter.unsafe.clean",
            "touch SHOULD_NOT_EXIST; cat",
        ]);
        git(&["config", "filter.unsafe.required", "true"]);
        git(&["config", "diff.unsafe.command", "touch SHOULD_NOT_EXIST"]);
        git(&["config", "core.fsmonitor", "touch SHOULD_NOT_EXIST"]);
        std::fs::write(root.path().join("sample"), "changed").unwrap();
        for name in ["git.status", "git.diff"] {
            let output = tool(name, root.path())
                .execute_validated(json!({}))
                .await
                .unwrap();
            assert_eq!(output["success"], true, "{output}");
            assert!(!root.path().join("SHOULD_NOT_EXIST").exists());
        }
        git(&["config", "--unset", "core.fsmonitor"]);
        let linked = root.path().join("linked");
        git(&["worktree", "add", "--detach", linked.to_str().unwrap()]);
        let output = tool("git.status", &linked)
            .execute_validated(json!({}))
            .await
            .unwrap();
        assert_eq!(output["success"], true, "{output}");
    }
    #[cfg(unix)]
    #[test]
    fn refuses_fifo_without_opening_it() {
        let root = tempfile::tempdir().unwrap();
        let fifo = root.path().join("fifo");
        assert!(std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success());
        assert!(read_file(&fifo)
            .unwrap_err()
            .to_string()
            .contains("regular file"));
    }
}
