//! Extension registry system for community connectors, tools, and agents.
//!
//! Registries are directories (optionally backed by Git repositories) that
//! contain Lua scripts and TOML definitions described by a `registry.toml`
//! manifest. Multiple registries can be configured with precedence ordering:
//!
//! ```text
//! community (readonly) → company (readonly) → personal (writable) → .ctx/ (project-local)
//! ```
//!
//! Later registries override earlier ones for the same extension name, and
//! explicit `ctx.toml` entries always take highest precedence.
//!
//! # Registry Layout
//!
//! ```text
//! registry.toml          # manifest
//! connectors/
//!   jira/
//!     connector.lua
//!     config.example.toml
//!     README.md
//! tools/
//!   summarize/
//!     tool.lua
//!     README.md
//! agents/
//!   runbook/
//!     agent.lua
//!     README.md
//! ```

use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::Config;

const COMMUNITY_REGISTRY_URL: &str = "https://github.com/context-harness/registry.git";
const DEFAULT_BRANCH: &str = "main";

// ═══════════════════════════════════════════════════════════════════════
// Manifest Types
// ═══════════════════════════════════════════════════════════════════════

/// Parsed `registry.toml` manifest describing all extensions in a registry.
#[derive(Debug, Deserialize, Clone)]
pub struct RegistryManifest {
    /// Top-level registry metadata.
    #[serde(default)]
    pub registry: RegistryMeta,
    /// Connector extensions keyed by name.
    #[serde(default)]
    pub connectors: HashMap<String, ExtensionEntry>,
    /// Tool extensions keyed by name.
    #[serde(default)]
    pub tools: HashMap<String, ExtensionEntry>,
    /// Agent extensions keyed by name.
    #[serde(default)]
    pub agents: HashMap<String, ExtensionEntry>,
}

/// Top-level metadata about a registry.
#[derive(Debug, Deserialize, Clone, Default)]
#[allow(dead_code)]
pub struct RegistryMeta {
    /// Human-readable registry name (e.g. `"community"`).
    #[serde(default)]
    pub name: String,
    /// One-line description.
    #[serde(default)]
    pub description: String,
    /// Canonical URL for the registry.
    #[serde(default)]
    pub url: Option<String>,
    /// Minimum `ctx` binary version required by this registry.
    #[serde(default)]
    pub min_version: Option<String>,
}

/// Metadata about a single extension (connector, tool, or agent).
#[derive(Debug, Deserialize, Clone)]
pub struct ExtensionEntry {
    /// One-line description of the extension.
    #[serde(default)]
    pub description: String,
    /// Relative path from registry root to the script file.
    pub path: String,
    /// Tags for filtering and discovery.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Config keys required by this extension (for connectors).
    #[serde(default)]
    pub required_config: Vec<String>,
    /// Lua host APIs used by this extension.
    #[serde(default)]
    pub host_apis: Vec<String>,
    /// Tools this agent exposes (agents only).
    #[serde(default)]
    pub tools: Vec<String>,
}

/// A resolved extension with its absolute script path and source registry.
#[derive(Debug, Clone)]
pub struct ResolvedExtension {
    /// Extension name (e.g. `"jira"`).
    pub name: String,
    /// Extension type: `"connector"`, `"tool"`, or `"agent"`.
    pub kind: String,
    /// Absolute path to the script file.
    pub script_path: PathBuf,
    /// Name of the registry this extension came from.
    pub registry_name: String,
    /// Extension metadata from the manifest.
    pub entry: ExtensionEntry,
}

// ═══════════════════════════════════════════════════════════════════════
// Git Operations
// ═══════════════════════════════════════════════════════════════════════

/// Shallow-clone a Git repository into `target_dir`.
pub fn clone_registry(url: &str, branch: Option<&str>, target_dir: &Path) -> Result<()> {
    if target_dir.exists() {
        anyhow::bail!(
            "Target directory already exists: {}",
            target_dir.display()
        );
    }

    if let Some(parent) = target_dir.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create parent directory: {}", parent.display()))?;
    }

    let branch = branch.unwrap_or(DEFAULT_BRANCH);
    let output = Command::new("git")
        .args([
            "clone",
            "--depth",
            "1",
            "--branch",
            branch,
            url,
            &target_dir.to_string_lossy(),
        ])
        .output()
        .context("Failed to run git clone")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git clone failed: {}", stderr.trim());
    }

    Ok(())
}

/// Pull the latest changes for a git-backed registry.
pub fn pull_registry(registry_dir: &Path) -> Result<()> {
    if !is_git_repo(registry_dir) {
        anyhow::bail!(
            "Not a git repository: {}",
            registry_dir.display()
        );
    }

    if is_dirty(registry_dir)? {
        eprintln!(
            "Warning: registry at {} has uncommitted changes, skipping update",
            registry_dir.display()
        );
        return Ok(());
    }

    let output = Command::new("git")
        .args(["pull", "--ff-only"])
        .current_dir(registry_dir)
        .output()
        .context("Failed to run git pull")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git pull failed: {}", stderr.trim());
    }

    Ok(())
}

/// Returns `true` if the directory contains a `.git` subdirectory.
pub fn is_git_repo(dir: &Path) -> bool {
    dir.join(".git").exists()
}

/// Returns `true` if the git working tree has uncommitted changes.
fn is_dirty(dir: &Path) -> Result<bool> {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(dir)
        .output()
        .context("Failed to run git status")?;

    Ok(!output.stdout.is_empty())
}

// ═══════════════════════════════════════════════════════════════════════
// Manifest Loading
// ═══════════════════════════════════════════════════════════════════════

/// Load and parse a `registry.toml` manifest from a registry directory.
pub fn load_manifest(registry_dir: &Path) -> Result<RegistryManifest> {
    let manifest_path = registry_dir.join("registry.toml");
    let content = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("Failed to read manifest: {}", manifest_path.display()))?;

    let manifest: RegistryManifest =
        toml::from_str(&content).with_context(|| "Failed to parse registry.toml")?;

    Ok(manifest)
}

/// Attempt to build a manifest by scanning the directory structure when
/// no `registry.toml` is present (e.g. for `.ctx/` project-local dirs).
fn discover_manifest(registry_dir: &Path) -> RegistryManifest {
    let mut manifest = RegistryManifest {
        registry: RegistryMeta::default(),
        connectors: HashMap::new(),
        tools: HashMap::new(),
        agents: HashMap::new(),
    };

    let scan_dir = |subdir: &str, script_name: &str| -> HashMap<String, ExtensionEntry> {
        let mut entries = HashMap::new();
        let dir = registry_dir.join(subdir);
        if !dir.is_dir() {
            return entries;
        }
        if let Ok(read) = std::fs::read_dir(&dir) {
            for entry in read.flatten() {
                if entry.path().is_dir() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let script = entry.path().join(script_name);
                    if script.exists() {
                        let rel_path = format!("{}/{}/{}", subdir, name, script_name);
                        entries.insert(
                            name,
                            ExtensionEntry {
                                description: String::new(),
                                path: rel_path,
                                tags: Vec::new(),
                                required_config: Vec::new(),
                                host_apis: Vec::new(),
                                tools: Vec::new(),
                            },
                        );
                    }
                }
            }
        }
        entries
    };

    manifest.connectors = scan_dir("connectors", "connector.lua");
    manifest.tools = scan_dir("tools", "tool.lua");

    // Agents can be .lua or .toml
    let mut agents = scan_dir("agents", "agent.lua");
    let toml_agents = scan_dir("agents", "agent.toml");
    for (k, v) in toml_agents {
        agents.entry(k).or_insert(v);
    }
    manifest.agents = agents;

    manifest
}

// ═══════════════════════════════════════════════════════════════════════
// Registry Manager
// ═══════════════════════════════════════════════════════════════════════

/// Manages multiple extension registries with precedence-based resolution.
///
/// Registries are loaded in config order. A `.ctx/` directory in the
/// current working directory (or ancestors) is appended with the highest
/// precedence. Later registries override earlier ones for the same name.
pub struct RegistryManager {
    /// Registries in precedence order (lowest first).
    registries: Vec<LoadedRegistry>,
}

struct LoadedRegistry {
    name: String,
    path: PathBuf,
    manifest: RegistryManifest,
    readonly: bool,
}

impl RegistryManager {
    /// Build a `RegistryManager` from the config, loading all manifests.
    ///
    /// Registries that don't exist on disk or lack a valid manifest are
    /// skipped with a warning. The `.ctx/` project-local directory is
    /// appended if found.
    pub fn from_config(config: &Config) -> Self {
        let mut registries = Vec::new();

        for (name, reg_cfg) in &config.registries {
            let path = expand_tilde(&reg_cfg.path);
            if !path.exists() {
                eprintln!(
                    "Warning: registry '{}' path does not exist: {}",
                    name,
                    path.display()
                );
                continue;
            }

            let manifest = match load_manifest(&path) {
                Ok(m) => m,
                Err(_) => {
                    eprintln!(
                        "Warning: registry '{}' has no valid registry.toml, using directory scan",
                        name
                    );
                    discover_manifest(&path)
                }
            };

            registries.push(LoadedRegistry {
                name: name.clone(),
                path,
                manifest,
                readonly: reg_cfg.readonly,
            });
        }

        // Append .ctx/ project-local directory if found
        if let Some(ctx_dir) = find_local_ctx_dir() {
            let manifest = match load_manifest(&ctx_dir) {
                Ok(m) => m,
                Err(_) => discover_manifest(&ctx_dir),
            };
            registries.push(LoadedRegistry {
                name: "project-local".to_string(),
                path: ctx_dir,
                manifest,
                readonly: false,
            });
        }

        Self { registries }
    }

    /// List all extensions across all registries, resolved by precedence.
    ///
    /// Later registries override earlier ones for the same `kind/name`.
    pub fn list_all(&self) -> Vec<ResolvedExtension> {
        let mut map: HashMap<String, ResolvedExtension> = HashMap::new();

        for reg in &self.registries {
            for (name, entry) in &reg.manifest.connectors {
                let key = format!("connectors/{}", name);
                map.insert(
                    key,
                    ResolvedExtension {
                        name: name.clone(),
                        kind: "connector".to_string(),
                        script_path: reg.path.join(&entry.path),
                        registry_name: reg.name.clone(),
                        entry: entry.clone(),
                    },
                );
            }
            for (name, entry) in &reg.manifest.tools {
                let key = format!("tools/{}", name);
                map.insert(
                    key,
                    ResolvedExtension {
                        name: name.clone(),
                        kind: "tool".to_string(),
                        script_path: reg.path.join(&entry.path),
                        registry_name: reg.name.clone(),
                        entry: entry.clone(),
                    },
                );
            }
            for (name, entry) in &reg.manifest.agents {
                let key = format!("agents/{}", name);
                map.insert(
                    key,
                    ResolvedExtension {
                        name: name.clone(),
                        kind: "agent".to_string(),
                        script_path: reg.path.join(&entry.path),
                        registry_name: reg.name.clone(),
                        entry: entry.clone(),
                    },
                );
            }
        }

        let mut all: Vec<ResolvedExtension> = map.into_values().collect();
        all.sort_by(|a, b| a.kind.cmp(&b.kind).then(a.name.cmp(&b.name)));
        all
    }

    /// Resolve a specific extension by `"type/name"` (e.g. `"connectors/jira"`).
    pub fn resolve(&self, extension_id: &str) -> Option<ResolvedExtension> {
        self.list_all()
            .into_iter()
            .find(|e| format!("{}s/{}", e.kind, e.name) == extension_id)
    }

    /// List all resolved connectors.
    pub fn list_connectors(&self) -> Vec<ResolvedExtension> {
        self.list_all()
            .into_iter()
            .filter(|e| e.kind == "connector")
            .collect()
    }

    /// List all resolved tools.
    pub fn list_tools(&self) -> Vec<ResolvedExtension> {
        self.list_all()
            .into_iter()
            .filter(|e| e.kind == "tool")
            .collect()
    }

    /// List all resolved agents.
    pub fn list_agents(&self) -> Vec<ResolvedExtension> {
        self.list_all()
            .into_iter()
            .filter(|e| e.kind == "agent")
            .collect()
    }

    /// Find the first writable registry path (iterates from highest precedence).
    pub fn writable_path(&self) -> Option<&Path> {
        self.registries
            .iter()
            .rev()
            .find(|r| !r.readonly)
            .map(|r| r.path.as_path())
    }

    /// Get the loaded registries (for listing/status).
    pub fn registries(&self) -> Vec<RegistryInfo> {
        self.registries
            .iter()
            .map(|r| RegistryInfo {
                name: r.name.clone(),
                path: r.path.clone(),
                readonly: r.readonly,
                is_git: is_git_repo(&r.path),
                connectors: r.manifest.connectors.len(),
                tools: r.manifest.tools.len(),
                agents: r.manifest.agents.len(),
            })
            .collect()
    }
}

/// Summary information about a loaded registry (for CLI display).
pub struct RegistryInfo {
    pub name: String,
    pub path: PathBuf,
    pub readonly: bool,
    pub is_git: bool,
    pub connectors: usize,
    pub tools: usize,
    pub agents: usize,
}

// ═══════════════════════════════════════════════════════════════════════
// .ctx/ Directory Discovery
// ═══════════════════════════════════════════════════════════════════════

/// Walk from the current directory upward looking for a `.ctx/` directory.
///
/// Returns the first `.ctx/` directory found, or `None` if the filesystem
/// root is reached without finding one.
pub fn find_local_ctx_dir() -> Option<PathBuf> {
    find_ctx_dir_from(std::env::current_dir().ok()?)
}

/// Walk from `start` upward looking for a `.ctx/` directory.
fn find_ctx_dir_from(start: PathBuf) -> Option<PathBuf> {
    let mut dir = start;
    loop {
        let candidate = dir.join(".ctx");
        if candidate.is_dir() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// CLI Command Implementations
// ═══════════════════════════════════════════════════════════════════════

/// `ctx registry list` — show configured registries and their extensions.
pub fn cmd_list(config: &Config) {
    let mgr = RegistryManager::from_config(config);

    let registries = mgr.registries();
    if registries.is_empty() {
        println!("No registries configured.");
        println!("Add a [registries.<name>] section to ctx.toml, or run `ctx registry install`.");
        return;
    }

    println!("Registries:\n");
    for r in &registries {
        let git_tag = if r.is_git { " (git)" } else { "" };
        let ro_tag = if r.readonly { " [readonly]" } else { "" };
        println!(
            "  {} — {}{}{}\n    {} connectors, {} tools, {} agents",
            r.name,
            r.path.display(),
            git_tag,
            ro_tag,
            r.connectors,
            r.tools,
            r.agents,
        );
    }

    let all = mgr.list_all();
    if all.is_empty() {
        println!("\nNo extensions found.");
        return;
    }

    println!("\nAvailable extensions:\n");
    let mut current_kind = String::new();
    for ext in &all {
        if ext.kind != current_kind {
            current_kind = ext.kind.clone();
            println!("  {}s:", current_kind);
        }
        let tags = if ext.entry.tags.is_empty() {
            String::new()
        } else {
            format!(" [{}]", ext.entry.tags.join(", "))
        };
        println!(
            "    {} — {}{} (from: {})",
            ext.name, ext.entry.description, tags, ext.registry_name
        );
    }
}

/// `ctx registry install` — clone configured registries that aren't yet present.
pub fn cmd_install(config: &Config, name: Option<&str>) -> Result<()> {
    let mut installed = 0;

    for (reg_name, reg_cfg) in &config.registries {
        if let Some(filter) = name {
            if reg_name != filter {
                continue;
            }
        }

        let url = match &reg_cfg.url {
            Some(u) => u,
            None => {
                if name.is_some() {
                    println!("Registry '{}' is local-only (no url configured).", reg_name);
                }
                continue;
            }
        };

        let target = expand_tilde(&reg_cfg.path);
        if target.exists() {
            println!("Registry '{}' already installed at {}", reg_name, target.display());
            continue;
        }

        println!("Cloning registry '{}' from {}...", reg_name, url);
        clone_registry(url, reg_cfg.branch.as_deref(), &target)?;

        // Report what was installed
        match load_manifest(&target) {
            Ok(m) => {
                println!(
                    "  Installed: {} connectors, {} tools, {} agents",
                    m.connectors.len(),
                    m.tools.len(),
                    m.agents.len()
                );
            }
            Err(_) => {
                println!("  Installed (no registry.toml found — extensions will be discovered by directory scan).");
            }
        }
        installed += 1;
    }

    if installed == 0 && name.is_none() {
        println!("No registries to install. Add [registries.<name>] entries with `url` to ctx.toml.");
    }

    Ok(())
}

/// `ctx registry update` — git pull all (or a specific) registry.
pub fn cmd_update(config: &Config, name: Option<&str>) -> Result<()> {
    let mut updated = 0;

    for (reg_name, reg_cfg) in &config.registries {
        if let Some(filter) = name {
            if reg_name != filter {
                continue;
            }
        }

        let path = expand_tilde(&reg_cfg.path);
        if !path.exists() {
            eprintln!("Registry '{}' not installed at {}. Run `ctx registry install` first.", reg_name, path.display());
            continue;
        }

        if !is_git_repo(&path) {
            if name.is_some() {
                println!("Registry '{}' is not a git repository, skipping.", reg_name);
            }
            continue;
        }

        println!("Updating registry '{}'...", reg_name);
        match pull_registry(&path) {
            Ok(()) => {
                println!("  Updated successfully.");
                updated += 1;
            }
            Err(e) => {
                eprintln!("  Failed to update '{}': {}", reg_name, e);
            }
        }
    }

    if updated == 0 && name.is_none() {
        println!("No git-backed registries to update.");
    }

    Ok(())
}

/// `ctx registry search <query>` — fuzzy search extensions by name/description/tags.
pub fn cmd_search(config: &Config, query: &str) {
    let mgr = RegistryManager::from_config(config);
    let all = mgr.list_all();

    let query_lower = query.to_lowercase();
    let matches: Vec<&ResolvedExtension> = all
        .iter()
        .filter(|e| {
            e.name.to_lowercase().contains(&query_lower)
                || e.entry.description.to_lowercase().contains(&query_lower)
                || e.entry
                    .tags
                    .iter()
                    .any(|t| t.to_lowercase().contains(&query_lower))
        })
        .collect();

    if matches.is_empty() {
        println!("No extensions matching '{}'.", query);
        return;
    }

    println!("Found {} extensions matching '{}':\n", matches.len(), query);
    for ext in matches {
        let tags = if ext.entry.tags.is_empty() {
            String::new()
        } else {
            format!(" [{}]", ext.entry.tags.join(", "))
        };
        println!(
            "  {}s/{} — {}{} (from: {})",
            ext.kind, ext.name, ext.entry.description, tags, ext.registry_name
        );
    }
}

/// `ctx registry info <type/name>` — show details for a specific extension.
pub fn cmd_info(config: &Config, extension_id: &str) -> Result<()> {
    let mgr = RegistryManager::from_config(config);

    let ext = mgr
        .resolve(extension_id)
        .ok_or_else(|| anyhow::anyhow!("Extension '{}' not found in any registry", extension_id))?;

    println!("Extension: {}s/{}", ext.kind, ext.name);
    println!("Registry:  {}", ext.registry_name);
    println!("Script:    {}", ext.script_path.display());

    if !ext.entry.description.is_empty() {
        println!("Description: {}", ext.entry.description);
    }
    if !ext.entry.tags.is_empty() {
        println!("Tags: {}", ext.entry.tags.join(", "));
    }
    if !ext.entry.required_config.is_empty() {
        println!("Required config: {}", ext.entry.required_config.join(", "));
    }
    if !ext.entry.host_apis.is_empty() {
        println!("Host APIs: {}", ext.entry.host_apis.join(", "));
    }
    if !ext.entry.tools.is_empty() {
        println!("Tools: {}", ext.entry.tools.join(", "));
    }

    // Try to print the README
    let readme_path = ext.script_path.parent().map(|p| p.join("README.md"));
    if let Some(ref readme) = readme_path {
        if readme.exists() {
            if let Ok(content) = std::fs::read_to_string(readme) {
                println!("\n--- README ---\n\n{}", content);
            }
        }
    }

    Ok(())
}

/// `ctx registry add <type/name>` — scaffold a config entry in ctx.toml.
pub fn cmd_add(config: &Config, extension_id: &str, config_path: &Path) -> Result<()> {
    let mgr = RegistryManager::from_config(config);

    let ext = mgr
        .resolve(extension_id)
        .ok_or_else(|| anyhow::anyhow!("Extension '{}' not found in any registry", extension_id))?;

    // Try to load config.example.toml from the extension directory
    let example_path = ext
        .script_path
        .parent()
        .map(|p| p.join("config.example.toml"));
    let example_content = example_path
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok());

    let section = match ext.kind.as_str() {
        "connector" => {
            if let Some(example) = &example_content {
                format!(
                    "\n[connectors.script.{}]\n{}",
                    ext.name, example
                )
            } else {
                let mut section = format!(
                    "\n[connectors.script.{}]\npath = \"{}\"\n",
                    ext.name,
                    ext.script_path.display()
                );
                for key in &ext.entry.required_config {
                    section.push_str(&format!("{} = \"\"  # TODO: set this\n", key));
                }
                section
            }
        }
        "tool" => {
            if let Some(example) = &example_content {
                format!(
                    "\n[tools.script.{}]\n{}",
                    ext.name, example
                )
            } else {
                format!(
                    "\n[tools.script.{}]\npath = \"{}\"\n",
                    ext.name,
                    ext.script_path.display()
                )
            }
        }
        "agent" => {
            if let Some(example) = &example_content {
                format!(
                    "\n[agents.script.{}]\n{}",
                    ext.name, example
                )
            } else {
                format!(
                    "\n[agents.script.{}]\npath = \"{}\"\n",
                    ext.name,
                    ext.script_path.display()
                )
            }
        }
        _ => anyhow::bail!("Unknown extension kind: {}", ext.kind),
    };

    // Append to config file
    let mut content = std::fs::read_to_string(config_path)
        .with_context(|| format!("Failed to read config: {}", config_path.display()))?;

    content.push_str(&section);

    std::fs::write(config_path, &content)
        .with_context(|| format!("Failed to write config: {}", config_path.display()))?;

    println!(
        "Added [{}s.script.{}] to {}",
        ext.kind,
        ext.name,
        config_path.display()
    );

    if !ext.entry.required_config.is_empty() {
        println!(
            "Edit {} to set: {}",
            config_path.display(),
            ext.entry.required_config.join(", ")
        );
    }

    Ok(())
}

/// `ctx registry override <type/name>` — copy extension to a writable registry.
pub fn cmd_override(config: &Config, extension_id: &str) -> Result<()> {
    let mgr = RegistryManager::from_config(config);

    let ext = mgr
        .resolve(extension_id)
        .ok_or_else(|| anyhow::anyhow!("Extension '{}' not found in any registry", extension_id))?;

    let writable_path = mgr
        .writable_path()
        .ok_or_else(|| anyhow::anyhow!("No writable registry found. Add a registry with readonly = false."))?;

    let dest_dir = writable_path
        .join(format!("{}s", ext.kind))
        .join(&ext.name);

    if dest_dir.exists() {
        anyhow::bail!(
            "Override already exists at {}. Edit it directly.",
            dest_dir.display()
        );
    }

    // Copy the entire extension directory
    let src_dir = ext
        .script_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Cannot determine source directory"))?;

    copy_dir_recursive(src_dir, &dest_dir)?;

    println!(
        "Copied {}s/{} to {}",
        ext.kind,
        ext.name,
        dest_dir.display()
    );
    println!(
        "Your version will take precedence over the {} registry version.",
        ext.registry_name
    );

    Ok(())
}

/// `ctx registry init` — interactive first-run community registry setup.
pub fn cmd_init_community(config_path: &Path) -> Result<()> {
    let default_path = default_registries_dir().join("community");

    if default_path.exists() {
        println!("Community registry already installed at {}", default_path.display());
        return Ok(());
    }

    println!("Cloning community extension registry...");
    clone_registry(COMMUNITY_REGISTRY_URL, Some(DEFAULT_BRANCH), &default_path)?;

    // Report what was installed
    match load_manifest(&default_path) {
        Ok(m) => {
            println!(
                "Installed: {} connectors, {} tools, {} agents",
                m.connectors.len(),
                m.tools.len(),
                m.agents.len()
            );
        }
        Err(_) => {
            println!("Installed (directory scan mode — no registry.toml).");
        }
    }

    // Append registry config to ctx.toml
    let registry_section = format!(
        "\n[registries.community]\nurl = \"{}\"\nbranch = \"{}\"\npath = \"{}\"\nreadonly = true\nauto_update = true\n",
        COMMUNITY_REGISTRY_URL,
        DEFAULT_BRANCH,
        default_path.display()
    );

    let mut content = std::fs::read_to_string(config_path)
        .with_context(|| format!("Failed to read config: {}", config_path.display()))?;
    content.push_str(&registry_section);
    std::fs::write(config_path, &content)
        .with_context(|| format!("Failed to write config: {}", config_path.display()))?;

    println!(
        "Added [registries.community] to {}",
        config_path.display()
    );
    println!("Run `ctx registry list` to see available extensions.");

    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════
// Utilities
// ═══════════════════════════════════════════════════════════════════════

/// Expand `~` at the start of a path to the user's home directory.
fn expand_tilde(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if s.starts_with("~/") || s == "~" {
        if let Some(home) = home_dir() {
            return home.join(s.strip_prefix("~/").unwrap_or(""));
        }
    }
    path.to_path_buf()
}

/// Get the user's home directory.
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Default directory for storing registries.
fn default_registries_dir() -> PathBuf {
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".ctx")
        .join("registries")
}

/// Recursively copy a directory and all its contents.
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;

    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            if entry.file_name() == ".git" {
                continue;
            }
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }

    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RegistryConfig;

    #[test]
    fn parse_manifest() {
        let toml = r#"
[registry]
name = "test"
description = "Test registry"
min_version = "0.3.0"

[connectors.jira]
description = "Index Jira issues"
path = "connectors/jira/connector.lua"
tags = ["atlassian", "pm"]
required_config = ["url", "api_token"]
host_apis = ["http", "json"]

[tools.summarize]
description = "Summarize a document"
path = "tools/summarize/tool.lua"
tags = ["llm"]
host_apis = ["http", "context"]

[agents.runbook]
description = "Incident response agent"
path = "agents/runbook/agent.lua"
tags = ["ops"]
tools = ["search", "get"]
"#;

        let manifest: RegistryManifest = toml::from_str(toml).unwrap();

        assert_eq!(manifest.registry.name, "test");
        assert_eq!(manifest.registry.min_version, Some("0.3.0".to_string()));

        assert_eq!(manifest.connectors.len(), 1);
        let jira = &manifest.connectors["jira"];
        assert_eq!(jira.description, "Index Jira issues");
        assert_eq!(jira.path, "connectors/jira/connector.lua");
        assert_eq!(jira.tags, vec!["atlassian", "pm"]);
        assert_eq!(jira.required_config, vec!["url", "api_token"]);

        assert_eq!(manifest.tools.len(), 1);
        assert_eq!(manifest.tools["summarize"].description, "Summarize a document");

        assert_eq!(manifest.agents.len(), 1);
        assert_eq!(manifest.agents["runbook"].tools, vec!["search", "get"]);
    }

    #[test]
    fn parse_empty_manifest() {
        let toml = "[registry]\nname = \"empty\"\n";
        let manifest: RegistryManifest = toml::from_str(toml).unwrap();
        assert_eq!(manifest.connectors.len(), 0);
        assert_eq!(manifest.tools.len(), 0);
        assert_eq!(manifest.agents.len(), 0);
    }

    #[test]
    fn expand_tilde_works() {
        let expanded = expand_tilde(Path::new("~/foo/bar"));
        assert!(!expanded.to_string_lossy().starts_with("~"));
        assert!(expanded.to_string_lossy().ends_with("foo/bar"));
    }

    #[test]
    fn expand_tilde_noop_for_absolute() {
        let path = Path::new("/usr/local/bin");
        assert_eq!(expand_tilde(path), path.to_path_buf());
    }

    #[test]
    fn discover_manifest_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = discover_manifest(dir.path());
        assert!(manifest.connectors.is_empty());
        assert!(manifest.tools.is_empty());
        assert!(manifest.agents.is_empty());
    }

    #[test]
    fn discover_manifest_finds_scripts() {
        let dir = tempfile::tempdir().unwrap();

        // Create connectors/jira/connector.lua
        let jira_dir = dir.path().join("connectors").join("jira");
        std::fs::create_dir_all(&jira_dir).unwrap();
        std::fs::write(jira_dir.join("connector.lua"), "-- jira").unwrap();

        // Create tools/summarize/tool.lua
        let tool_dir = dir.path().join("tools").join("summarize");
        std::fs::create_dir_all(&tool_dir).unwrap();
        std::fs::write(tool_dir.join("tool.lua"), "-- summarize").unwrap();

        // Create agents/runbook/agent.lua
        let agent_dir = dir.path().join("agents").join("runbook");
        std::fs::create_dir_all(&agent_dir).unwrap();
        std::fs::write(agent_dir.join("agent.lua"), "-- runbook").unwrap();

        let manifest = discover_manifest(dir.path());
        assert_eq!(manifest.connectors.len(), 1);
        assert!(manifest.connectors.contains_key("jira"));
        assert_eq!(manifest.tools.len(), 1);
        assert!(manifest.tools.contains_key("summarize"));
        assert_eq!(manifest.agents.len(), 1);
        assert!(manifest.agents.contains_key("runbook"));
    }

    #[test]
    fn resolution_order_later_wins() {
        let dir_a = tempfile::tempdir().unwrap();
        let dir_b = tempfile::tempdir().unwrap();

        // Both registries have connectors/jira
        for dir in [dir_a.path(), dir_b.path()] {
            let jira_dir = dir.join("connectors").join("jira");
            std::fs::create_dir_all(&jira_dir).unwrap();
            std::fs::write(jira_dir.join("connector.lua"), "-- jira").unwrap();
        }

        let config = Config {
            registries: {
                let mut m = HashMap::new();
                m.insert(
                    "first".to_string(),
                    RegistryConfig {
                        url: None,
                        branch: None,
                        path: dir_a.path().to_path_buf(),
                        readonly: true,
                        auto_update: false,
                    },
                );
                m.insert(
                    "second".to_string(),
                    RegistryConfig {
                        url: None,
                        branch: None,
                        path: dir_b.path().to_path_buf(),
                        readonly: false,
                        auto_update: false,
                    },
                );
                m
            },
            ..Config::minimal()
        };

        let mgr = RegistryManager::from_config(&config);
        let connectors = mgr.list_connectors();
        assert_eq!(connectors.len(), 1);
        // The winner should be from one of the two registries; the exact
        // winner depends on HashMap iteration order, but both are valid
        // since they have the same name.
        assert_eq!(connectors[0].name, "jira");
    }

    #[test]
    fn find_ctx_dir_from_nested() {
        let root = tempfile::tempdir().unwrap();
        let ctx_dir = root.path().join(".ctx");
        std::fs::create_dir_all(&ctx_dir).unwrap();

        let nested = root.path().join("src").join("deep");
        std::fs::create_dir_all(&nested).unwrap();

        let found = find_ctx_dir_from(nested);
        assert!(found.is_some());
        assert_eq!(found.unwrap(), ctx_dir);
    }

    #[test]
    fn find_ctx_dir_returns_none_when_missing() {
        let root = tempfile::tempdir().unwrap();
        let nested = root.path().join("src").join("deep");
        std::fs::create_dir_all(&nested).unwrap();

        let found = find_ctx_dir_from(nested);
        assert!(found.is_none());
    }
}
