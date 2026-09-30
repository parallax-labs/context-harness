//! Declarative tool-binding contracts from SPEC-0023.
//!
//! This module is deliberately side-effect free: parsing resources and registering
//! trusted factories does not load scripts, start processes, open a database, or
//! resolve secrets.

use crate::{agent_resource::Capability, traits::Tool};
use anyhow::{ensure, Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    path::{Component, Path, PathBuf},
    sync::Arc,
};

const RESOURCE_SCHEMA_VERSION: u32 = 1;
const MAX_OUTPUT_BYTES: u64 = 1024 * 1024;

/// A versioned standalone tool resource.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolBindingResource {
    pub schema_version: u32,
    pub tool: ToolBindingSettings,
    #[serde(default, skip_serializing_if = "toml::Table::is_empty")]
    pub config: toml::Table,
    #[serde(default, skip_serializing_if = "toml::Table::is_empty")]
    pub fixed: toml::Table,
    #[serde(default, skip_serializing_if = "BindingRestrictions::is_empty")]
    pub restrictions: BindingRestrictions,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolBindingSettings {
    pub name: String,
    pub implementation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, rename = "override")]
    pub override_existing: bool,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct BindingRestrictions {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_bytes: Option<u64>,
}

impl BindingRestrictions {
    fn is_empty(&self) -> bool {
        self.paths.is_empty() && self.sources.is_empty() && self.max_output_bytes.is_none()
    }

    fn validate(&self) -> Result<()> {
        let mut normalized_paths = HashSet::new();
        for path in &self.paths {
            ensure!(
                normalized_paths.insert(normalize_resource_path(path)?),
                "duplicate restricted path"
            );
        }
        ensure!(
            self.sources.iter().all(|source| {
                !source.trim().is_empty() && source.len() <= 1024 && !source.contains('\0')
            }),
            "invalid restricted source"
        );
        ensure!(
            self.sources.iter().collect::<HashSet<_>>().len() == self.sources.len(),
            "duplicate restricted source"
        );
        if let Some(limit) = self.max_output_bytes {
            ensure!(
                (1..=MAX_OUTPUT_BYTES).contains(&limit),
                "max_output_bytes must be between 1 and {MAX_OUTPUT_BYTES}"
            );
        }
        Ok(())
    }
}

impl ToolBindingResource {
    pub fn parse(content: &str) -> Result<Self> {
        let resource: Self = toml::from_str(content).context("invalid tool resource TOML")?;
        resource.validate()?;
        Ok(resource)
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == RESOURCE_SCHEMA_VERSION,
            "unsupported tool resource schema version"
        );
        validate_identifier(&self.tool.name, "tool name")?;
        validate_identifier(&self.tool.implementation, "tool implementation")?;
        ensure!(
            !reserved_public_name(&self.tool.name),
            "reserved public tool name"
        );
        if let Some(description) = &self.tool.description {
            ensure!(
                !description.trim().is_empty() && description.len() <= 1024,
                "tool description must contain 1-1024 bytes"
            );
        }
        self.restrictions.validate()
    }

    /// Hash parsed, default-expanded content. Implementation and derived-schema
    /// identity are added later when the resource is resolved against a catalog.
    pub fn resource_version(&self) -> Result<String> {
        Ok(format!(
            "sha256:{:x}",
            Sha256::digest(serde_json::to_vec(self)?)
        ))
    }
}

fn validate_identifier(value: &str, label: &str) -> Result<()> {
    let mut chars = value.chars();
    ensure!(
        value.len() <= 128
            && chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
            && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')),
        "invalid {label}"
    );
    Ok(())
}

fn reserved_public_name(name: &str) -> bool {
    matches!(name, "search" | "get" | "process.exec" | "agent.invoke")
        || ["workspace.", "git.", "runtime.", "mcp."]
            .iter()
            .any(|prefix| name.starts_with(prefix))
}

fn normalize_resource_path(value: &str) -> Result<PathBuf> {
    ensure!(
        !value.is_empty() && !value.contains('\0'),
        "invalid restricted path"
    );
    let path = Path::new(value);
    ensure!(!path.is_absolute(), "restricted paths must be relative");
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                anyhow::bail!("restricted paths cannot escape the workspace")
            }
        }
    }
    ensure!(
        !normalized.as_os_str().is_empty(),
        "invalid restricted path"
    );
    Ok(normalized)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RestrictionKind {
    Paths,
    Sources,
    MaxOutputBytes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolTrustClass {
    Builtin,
    Compiled,
    LuaPrivileged,
    McpExternal,
}

/// Immutable, side-effect-free metadata supplied by a trusted implementation.
#[derive(Debug, Clone, Serialize)]
pub struct ToolImplementationDescriptor {
    pub id: String,
    pub version: String,
    pub implementation_description: String,
    pub default_public_description: String,
    pub config_schema: Value,
    pub input_schema: Value,
    pub capabilities: Vec<Capability>,
    pub supported_restrictions: Vec<RestrictionKind>,
    pub trust_class: ToolTrustClass,
}

impl ToolImplementationDescriptor {
    pub fn validate(&self) -> Result<()> {
        validate_identifier(&self.id, "implementation id")?;
        ensure!(
            self.id.starts_with("builtin.")
                || self.id.starts_with("rust.")
                || self.id.starts_with("lua.")
                || self.id.starts_with("mcp."),
            "implementation id uses an unowned namespace"
        );
        ensure!(
            !self.version.trim().is_empty() && self.version.len() <= 256,
            "implementation version must contain 1-256 bytes"
        );
        for description in [
            &self.implementation_description,
            &self.default_public_description,
        ] {
            ensure!(
                !description.trim().is_empty() && description.len() <= 1024,
                "implementation descriptions must contain 1-1024 bytes"
            );
        }
        validate_object_schema(&self.config_schema, "configuration")?;
        validate_object_schema(&self.input_schema, "input")?;
        ensure!(
            self.capabilities.iter().collect::<HashSet<_>>().len() == self.capabilities.len(),
            "duplicate implementation capability"
        );
        ensure!(
            self.supported_restrictions
                .iter()
                .collect::<HashSet<_>>()
                .len()
                == self.supported_restrictions.len(),
            "duplicate supported restriction"
        );
        Ok(())
    }
}

fn validate_object_schema(schema: &Value, label: &str) -> Result<()> {
    ensure!(schema.is_object(), "{label} schema must be an object");
    ensure!(
        schema.get("type").and_then(Value::as_str) == Some("object"),
        "{label} schema must declare object type"
    );
    Ok(())
}

/// Host-issued authority. Resource declarations can only narrow these grants.
#[derive(Debug, Clone)]
pub struct HostToolAuthority {
    workspace_root: PathBuf,
    capabilities: Vec<Capability>,
    paths: Vec<PathBuf>,
    sources: Vec<String>,
}

impl HostToolAuthority {
    pub fn new(workspace_root: &Path, capabilities: Vec<Capability>) -> Result<Self> {
        let workspace_root = workspace_root
            .canonicalize()
            .context("canonicalizing tool-authority workspace root")?;
        ensure!(
            capabilities.iter().collect::<HashSet<_>>().len() == capabilities.len(),
            "duplicate host capability"
        );
        Ok(Self {
            workspace_root,
            capabilities,
            paths: Vec::new(),
            sources: Vec::new(),
        })
    }

    pub fn enroll_path(&mut self, path: &Path) -> Result<()> {
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.workspace_root.join(path)
        }
        .canonicalize()
        .context("canonicalizing enrolled tool path")?;
        ensure!(
            path.starts_with(&self.workspace_root),
            "enrolled tool path escapes the workspace"
        );
        ensure!(!self.paths.contains(&path), "duplicate enrolled tool path");
        self.paths.push(path);
        Ok(())
    }

    pub fn enroll_source(&mut self, source: String) -> Result<()> {
        ensure!(
            !source.trim().is_empty() && source.len() <= 1024 && !source.contains('\0'),
            "invalid enrolled source"
        );
        ensure!(!self.sources.contains(&source), "duplicate enrolled source");
        self.sources.push(source);
        Ok(())
    }

    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    pub fn capabilities(&self) -> &[Capability] {
        &self.capabilities
    }

    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    pub fn sources(&self) -> &[String] {
        &self.sources
    }
}

/// Owned input to a trusted implementation factory.
pub struct ToolBindingRequest {
    pub resource: ToolBindingResource,
    pub authority: Arc<HostToolAuthority>,
}

/// Trusted executable registration. Binding may initialize local resources but
/// discovery and descriptor access remain side-effect free.
#[async_trait]
pub trait ToolImplementationFactory: Send + Sync {
    fn descriptor(&self) -> &ToolImplementationDescriptor;
    async fn bind(&self, request: ToolBindingRequest) -> Result<Box<dyn Tool>>;
}

#[derive(Default)]
pub struct ToolImplementationCatalog {
    factories: BTreeMap<String, Arc<dyn ToolImplementationFactory>>,
}

impl ToolImplementationCatalog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, factory: Arc<dyn ToolImplementationFactory>) -> Result<()> {
        let descriptor = factory.descriptor();
        descriptor.validate()?;
        ensure!(
            !self.factories.contains_key(&descriptor.id),
            "duplicate tool implementation '{}'",
            descriptor.id
        );
        self.factories.insert(descriptor.id.clone(), factory);
        Ok(())
    }

    pub fn find(&self, id: &str) -> Option<&Arc<dyn ToolImplementationFactory>> {
        self.factories.get(id)
    }

    pub fn descriptors(&self) -> impl Iterator<Item = &ToolImplementationDescriptor> {
        self.factories.values().map(|factory| factory.descriptor())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RESOURCE: &str = r#"
schema_version = 1
[tool]
name = "release.read"
implementation = "builtin.scoped_file_read"
description = "Read release files"
[config]
root = "release"
[fixed]
max_bytes = 65536
[restrictions]
paths = ["release"]
max_output_bytes = 65536
"#;

    #[test]
    fn resource_is_strict_validated_and_content_versioned() {
        let first = ToolBindingResource::parse(RESOURCE).unwrap();
        let second = ToolBindingResource::parse(&format!("\n{RESOURCE}\n# comment\n")).unwrap();
        assert_eq!(first.tool.name, "release.read");
        assert_eq!(
            first.resource_version().unwrap(),
            second.resource_version().unwrap()
        );
        assert!(ToolBindingResource::parse(&RESOURCE.replace(
            "description = \"Read release files\"",
            "description = \"Read release files\"\nunknown = true"
        ))
        .is_err());
    }

    #[test]
    fn resource_rejects_reserved_names_and_invalid_restrictions() {
        for invalid in [
            RESOURCE.replace("release.read", "workspace.read"),
            RESOURCE.replace("paths = [\"release\"]", "paths = [\"../release\"]"),
            RESOURCE.replace(
                "paths = [\"release\"]",
                "paths = [\"release\", \"./release\"]",
            ),
            RESOURCE.replace("max_output_bytes = 65536", "max_output_bytes = 0"),
            RESOURCE.replace("schema_version = 1", "schema_version = 2"),
        ] {
            assert!(ToolBindingResource::parse(&invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn host_authority_cannot_enroll_paths_outside_workspace() {
        let workspace = tempfile::TempDir::new().unwrap();
        let outside = tempfile::TempDir::new().unwrap();
        std::fs::create_dir(workspace.path().join("allowed")).unwrap();
        let mut authority =
            HostToolAuthority::new(workspace.path(), vec![Capability::ReadOnly]).unwrap();
        authority.enroll_path(Path::new("allowed")).unwrap();
        assert!(authority.enroll_path(outside.path()).is_err());
        assert_eq!(authority.paths().len(), 1);
    }

    #[test]
    fn descriptor_validation_rejects_ambiguous_contracts() {
        let mut descriptor = ToolImplementationDescriptor {
            id: "builtin.reader".into(),
            version: "1".into(),
            implementation_description: "Read scoped files".into(),
            default_public_description: "Read a file".into(),
            config_schema: serde_json::json!({"type":"object"}),
            input_schema: serde_json::json!({"type":"object"}),
            capabilities: vec![Capability::ReadOnly],
            supported_restrictions: vec![RestrictionKind::Paths],
            trust_class: ToolTrustClass::Builtin,
        };
        descriptor.validate().unwrap();
        descriptor.capabilities.push(Capability::ReadOnly);
        assert!(descriptor.validate().is_err());
        descriptor.capabilities.pop();
        descriptor.input_schema = serde_json::json!({"type":"string"});
        assert!(descriptor.validate().is_err());
    }
}
