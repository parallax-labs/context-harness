//! Declarative tool-binding contracts from SPEC-0023.
//!
//! This module is deliberately side-effect free: parsing resources and registering
//! trusted factories does not load scripts, start processes, open a database, or
//! resolve secrets.

use crate::{
    agent_resource::{Capability, ResourceDirectory, ResourceScope},
    config::{Config, ResolvedConfig},
    ctx_dirs::{self, ConfigSourceKind},
    traits::{Tool, ToolRegistry},
};
use anyhow::{bail, ensure, Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    fs,
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

/// Choose tool resource paths using the same explicit-config isolation as agents.
pub fn resource_directories(
    resolved: &ResolvedConfig,
    root: &Path,
    global: &Path,
) -> Vec<ResourceDirectory> {
    if matches!(
        resolved.source,
        ConfigSourceKind::Explicit | ConfigSourceKind::Env
    ) {
        return resolved
            .path
            .as_ref()
            .map(|path| {
                let path = if path.is_absolute() {
                    path.clone()
                } else {
                    root.join(path)
                };
                ResourceDirectory {
                    path: path.parent().unwrap_or(root).join("tools"),
                    scope: ResourceScope::Explicit,
                }
            })
            .into_iter()
            .collect();
    }
    vec![
        ResourceDirectory {
            path: global.join("tools"),
            scope: ResourceScope::Global,
        },
        ResourceDirectory {
            path: root.join(".ctx/tools"),
            scope: ResourceScope::Workspace,
        },
    ]
}

pub fn cli_resource_directories(resolved: &ResolvedConfig) -> Result<Vec<ResourceDirectory>> {
    Ok(resource_directories(
        resolved,
        &std::env::current_dir()?,
        &ctx_dirs::config_dir(),
    ))
}

#[derive(Debug, Clone, Serialize)]
pub struct LoadedToolResource {
    pub path: PathBuf,
    pub scope: ResourceScope,
    pub resource_version: String,
    pub definition: ToolBindingResource,
}

/// Load sorted layers and replace whole resources only when explicitly requested.
pub fn load_resources(
    directories: &[ResourceDirectory],
    config: &Config,
) -> Result<BTreeMap<String, LoadedToolResource>> {
    load_resources_selected(directories, config, None)
}

pub fn load_named_resource(
    directories: &[ResourceDirectory],
    config: &Config,
    name: &str,
) -> Result<BTreeMap<String, LoadedToolResource>> {
    validate_identifier(name, "tool name")?;
    load_resources_selected(directories, config, Some(name))
}

fn load_resources_selected(
    directories: &[ResourceDirectory],
    config: &Config,
    selected: Option<&str>,
) -> Result<BTreeMap<String, LoadedToolResource>> {
    let mut resources: BTreeMap<String, LoadedToolResource> = BTreeMap::new();
    let mut visited = HashSet::new();
    for directory in directories {
        let canonical = match directory.path.canonicalize() {
            Ok(path) => path,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(error).with_context(|| format!("reading {}", directory.path.display()))
            }
        };
        if !visited.insert(canonical.clone()) {
            continue;
        }
        let mut paths = fs::read_dir(&canonical)
            .with_context(|| format!("reading {}", canonical.display()))?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        paths.sort();
        let mut names = HashSet::new();
        for path in paths.into_iter().filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "toml")
        }) {
            let content =
                fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
            if let Some(selected) = selected {
                let declared_name =
                    toml::from_str::<toml::Value>(&content)
                        .ok()
                        .and_then(|value| {
                            value
                                .get("tool")
                                .and_then(|tool| tool.get("name"))
                                .and_then(toml::Value::as_str)
                                .map(str::to_owned)
                        });
                if declared_name.as_deref() != Some(selected) {
                    continue;
                }
            }
            let definition = ToolBindingResource::parse(&content)
                .with_context(|| format!("tool resource {}", path.display()))?;
            let name = &definition.tool.name;
            ensure!(
                names.insert(name.clone()),
                "duplicate tool binding '{name}' in {}",
                canonical.display()
            );
            ensure!(
                !config.tools.script.contains_key(name),
                "tool resource {} conflicts with legacy Lua tool '{name}'; rename or migrate it",
                path.display()
            );
            if let Some(previous) = resources.get(name) {
                ensure!(
                    definition.tool.override_existing,
                    "tool binding '{name}' in {} shadows {}; set tool.override = true to replace it",
                    path.display(),
                    previous.path.display()
                );
            }
            let resource_version = definition.resource_version()?;
            resources.insert(
                name.clone(),
                LoadedToolResource {
                    path,
                    scope: directory.scope,
                    resource_version,
                    definition,
                },
            );
        }
    }
    Ok(resources)
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

#[derive(Debug, Clone, Serialize)]
pub struct ResolvedToolBinding {
    pub name: String,
    pub description: String,
    pub implementation_description: String,
    pub implementation_id: String,
    pub implementation_version: String,
    pub trust_class: ToolTrustClass,
    pub capabilities: Vec<Capability>,
    pub public_schema: Value,
    pub config: Value,
    pub fixed: Value,
    pub restrictions: BindingRestrictions,
    pub binding_version: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResolvedToolResource {
    pub path: PathBuf,
    pub scope: ResourceScope,
    pub resource_version: String,
    pub binding: ResolvedToolBinding,
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
    pub resolved: ResolvedToolBinding,
    pub authority: Arc<HostToolAuthority>,
}

/// Trusted executable registration. Binding may initialize local resources but
/// discovery and descriptor access remain side-effect free.
#[async_trait]
pub trait ToolImplementationFactory: Send + Sync {
    fn descriptor(&self) -> &ToolImplementationDescriptor;
    /// Implementation-specific checks that require no execution or secret access.
    fn validate_binding(&self, _resource: &ToolBindingResource) -> Result<()> {
        Ok(())
    }
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

    pub fn resolve(&self, resource: &ToolBindingResource) -> Result<ResolvedToolBinding> {
        let factory = self.find(&resource.tool.implementation).with_context(|| {
            format!(
                "unknown tool implementation '{}'",
                resource.tool.implementation
            )
        })?;
        let descriptor = factory.descriptor();
        for restriction in resource.restrictions.kinds() {
            ensure!(
                descriptor.supported_restrictions.contains(&restriction),
                "implementation '{}' cannot enforce restriction '{restriction:?}'",
                descriptor.id
            );
        }
        let config = toml_table_to_json(&resource.config)?;
        let fixed = toml_table_to_json(&resource.fixed)?;
        validate_schema_value(&descriptor.config_schema, &config, "config")?;
        validate_fixed(&descriptor.input_schema, &fixed)?;
        factory.validate_binding(resource)?;
        let public_schema = derive_public_schema(&descriptor.input_schema, &resource.fixed)?;
        let description = resource
            .tool
            .description
            .clone()
            .unwrap_or_else(|| descriptor.default_public_description.clone());
        let identity = serde_json::json!({
            "resource": resource,
            "implementation_id": descriptor.id,
            "implementation_version": descriptor.version,
            "input_schema": descriptor.input_schema,
            "public_schema": public_schema,
            "config": config,
            "fixed": fixed,
            "restrictions": resource.restrictions,
        });
        let binding_version = format!(
            "sha256:{:x}",
            Sha256::digest(serde_json::to_vec(&identity)?)
        );
        Ok(ResolvedToolBinding {
            name: resource.tool.name.clone(),
            description,
            implementation_description: descriptor.implementation_description.clone(),
            implementation_id: descriptor.id.clone(),
            implementation_version: descriptor.version.clone(),
            trust_class: descriptor.trust_class,
            capabilities: descriptor.capabilities.clone(),
            public_schema,
            config,
            fixed,
            restrictions: resource.restrictions.clone(),
            binding_version,
        })
    }
}

pub fn resolve_resources(
    loaded: BTreeMap<String, LoadedToolResource>,
    catalog: &ToolImplementationCatalog,
) -> Result<BTreeMap<String, ResolvedToolResource>> {
    loaded
        .into_iter()
        .map(|(name, loaded)| {
            let binding = catalog
                .resolve(&loaded.definition)
                .with_context(|| format!("resolving tool resource {}", loaded.path.display()))?;
            Ok((
                name,
                ResolvedToolResource {
                    path: loaded.path,
                    scope: loaded.scope,
                    resource_version: loaded.resource_version,
                    binding,
                },
            ))
        })
        .collect()
}

struct CoreFactory {
    descriptor: ToolImplementationDescriptor,
}

#[async_trait]
impl ToolImplementationFactory for CoreFactory {
    fn descriptor(&self) -> &ToolImplementationDescriptor {
        &self.descriptor
    }

    fn validate_binding(&self, resource: &ToolBindingResource) -> Result<()> {
        match self.descriptor.id.as_str() {
            "builtin.scoped_file_read" => {
                let root = resource
                    .config
                    .get("root")
                    .and_then(toml::Value::as_str)
                    .context("config.root must be a string")?;
                let root = normalize_resource_path(root)?;
                ensure!(
                    resource
                        .restrictions
                        .paths
                        .iter()
                        .map(|path| normalize_resource_path(path))
                        .collect::<Result<Vec<_>>>()?
                        .contains(&root),
                    "config.root must be included in restrictions.paths"
                );
            }
            "builtin.retrieval.search" | "builtin.retrieval.get" => {
                let source = resource
                    .config
                    .get("source")
                    .and_then(toml::Value::as_str)
                    .context("config.source must be a string")?;
                ensure!(
                    resource
                        .restrictions
                        .sources
                        .iter()
                        .any(|allowed| allowed == source),
                    "config.source must be included in restrictions.sources"
                );
            }
            _ => {}
        }
        Ok(())
    }

    async fn bind(&self, request: ToolBindingRequest) -> Result<Box<dyn Tool>> {
        match self.descriptor.id.as_str() {
            "builtin.scoped_file_read" => Ok(Box::new(ScopedFileRead::bind(
                request,
                self.descriptor.input_schema.clone(),
            )?)),
            _ => bail!(
                "tool implementation '{}' is not connected to runtime dispatch yet",
                self.descriptor.id
            ),
        }
    }
}

pub fn core_catalog() -> Result<ToolImplementationCatalog> {
    let mut catalog = ToolImplementationCatalog::new();
    for descriptor in [
        ToolImplementationDescriptor {
            id: "builtin.scoped_file_read".into(),
            version: format!("{}:1", env!("CARGO_PKG_VERSION")),
            implementation_description:
                "Read bounded UTF-8 files through a host-scoped path accessor".into(),
            default_public_description: "Read a scoped UTF-8 file".into(),
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {"root": {"type": "string", "minLength": 1}},
                "required": ["root"],
                "additionalProperties": false
            }),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "minLength": 1},
                    "max_bytes": {"type": "integer", "minimum": 1, "maximum": 1048576}
                },
                "required": ["path"],
                "additionalProperties": false
            }),
            capabilities: vec![Capability::ReadOnly],
            supported_restrictions: vec![RestrictionKind::Paths, RestrictionKind::MaxOutputBytes],
            trust_class: ToolTrustClass::Builtin,
        },
        ToolImplementationDescriptor {
            id: "builtin.retrieval.search".into(),
            version: format!("{}:1", env!("CARGO_PKG_VERSION")),
            implementation_description:
                "Search indexed context through a host-scoped source accessor".into(),
            default_public_description: "Search scoped indexed context".into(),
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {"source": {"type": "string", "minLength": 1, "maxLength": 1024}},
                "required": ["source"],
                "additionalProperties": false
            }),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "minLength": 1, "maxLength": 8192},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 100}
                },
                "required": ["query"],
                "additionalProperties": false
            }),
            capabilities: vec![Capability::ReadOnly],
            supported_restrictions: vec![RestrictionKind::Sources, RestrictionKind::MaxOutputBytes],
            trust_class: ToolTrustClass::Builtin,
        },
        ToolImplementationDescriptor {
            id: "builtin.retrieval.get".into(),
            version: format!("{}:1", env!("CARGO_PKG_VERSION")),
            implementation_description:
                "Retrieve indexed context through a host-scoped source accessor".into(),
            default_public_description: "Get a scoped indexed document".into(),
            config_schema: serde_json::json!({
                "type": "object",
                "properties": {"source": {"type": "string", "minLength": 1, "maxLength": 1024}},
                "required": ["source"],
                "additionalProperties": false
            }),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {"id": {"type": "string", "minLength": 1, "maxLength": 128}},
                "required": ["id"],
                "additionalProperties": false
            }),
            capabilities: vec![Capability::ReadOnly],
            supported_restrictions: vec![RestrictionKind::Sources, RestrictionKind::MaxOutputBytes],
            trust_class: ToolTrustClass::Builtin,
        },
    ] {
        catalog.register(Arc::new(CoreFactory { descriptor }))?;
    }
    Ok(catalog)
}

/// Backwards-compatible name for callers that only inspect descriptors.
pub fn core_metadata_catalog() -> Result<ToolImplementationCatalog> {
    core_catalog()
}

pub async fn bind_resources(
    loaded: &BTreeMap<String, LoadedToolResource>,
    catalog: &ToolImplementationCatalog,
    authority: Arc<HostToolAuthority>,
) -> Result<ToolRegistry> {
    let mut registry = ToolRegistry::new();
    for resource in loaded.values() {
        let resolved = catalog.resolve(&resource.definition)?;
        let factory = catalog
            .find(&resolved.implementation_id)
            .context("resolved implementation disappeared from catalog")?;
        let tool = factory
            .bind(ToolBindingRequest {
                resource: resource.definition.clone(),
                resolved,
                authority: authority.clone(),
            })
            .await
            .with_context(|| format!("binding tool resource {}", resource.path.display()))?;
        ensure!(
            registry.find(tool.name()).is_none(),
            "duplicate bound tool '{}'",
            tool.name()
        );
        registry.register(tool);
    }
    Ok(registry)
}

struct ScopedFileRead {
    binding: ResolvedToolBinding,
    input_schema: Value,
    fixed: serde_json::Map<String, Value>,
    root: PathBuf,
}

impl ScopedFileRead {
    fn bind(request: ToolBindingRequest, input_schema: Value) -> Result<Self> {
        let root = request
            .resource
            .config
            .get("root")
            .and_then(toml::Value::as_str)
            .context("config.root must be a string")?;
        let root = request
            .authority
            .workspace_root()
            .join(normalize_resource_path(root)?)
            .canonicalize()
            .context("canonicalizing scoped file root")?;
        ensure!(root.is_dir(), "scoped file root must be a directory");
        ensure!(
            request
                .authority
                .paths()
                .iter()
                .any(|granted| root.starts_with(granted)),
            "scoped file root is not granted by the host"
        );
        let fixed = request
            .resolved
            .fixed
            .as_object()
            .context("fixed arguments must be an object")?
            .clone();
        Ok(Self {
            binding: request.resolved,
            input_schema,
            fixed,
            root,
        })
    }

    fn effective_arguments(&self, arguments: Value) -> Result<serde_json::Map<String, Value>> {
        validate_schema_value(&self.binding.public_schema, &arguments, "arguments")?;
        let mut arguments = arguments
            .as_object()
            .context("tool arguments must be an object")?
            .clone();
        for (name, value) in &self.fixed {
            ensure!(
                !arguments.contains_key(name),
                "fixed argument '{name}' cannot be overridden"
            );
            arguments.insert(name.clone(), value.clone());
        }
        validate_schema_value(
            &self.input_schema,
            &Value::Object(arguments.clone()),
            "arguments",
        )?;
        Ok(arguments)
    }
}

#[async_trait]
impl Tool for ScopedFileRead {
    fn capabilities(&self) -> Option<Vec<Capability>> {
        Some(self.binding.capabilities.clone())
    }

    fn name(&self) -> &str {
        &self.binding.name
    }

    fn description(&self) -> &str {
        &self.binding.description
    }

    fn parameters_schema(&self) -> Value {
        self.binding.public_schema.clone()
    }

    fn validate_arguments(&self, arguments: &Value) -> Result<()> {
        validate_schema_value(&self.binding.public_schema, arguments, "arguments")
    }

    async fn execute(
        &self,
        arguments: Value,
        _context: &crate::traits::ToolContext,
    ) -> Result<Value> {
        let arguments = self.effective_arguments(arguments)?;
        let path = arguments
            .get("path")
            .and_then(Value::as_str)
            .context("path must be a string")?;
        let relative = normalize_resource_path(path)?;
        let path = self
            .root
            .join(&relative)
            .canonicalize()
            .context("canonicalizing scoped file")?;
        ensure!(
            path.starts_with(&self.root),
            "scoped file path escapes its root"
        );
        ensure!(path.is_file(), "scoped file path must be a regular file");
        let requested = arguments
            .get("max_bytes")
            .and_then(Value::as_u64)
            .unwrap_or(MAX_OUTPUT_BYTES);
        let limit = requested.min(
            self.binding
                .restrictions
                .max_output_bytes
                .unwrap_or(MAX_OUTPUT_BYTES),
        );
        let metadata = tokio::fs::metadata(&path).await?;
        ensure!(metadata.len() <= limit, "scoped file exceeds output limit");
        let bytes = tokio::fs::read(&path).await?;
        ensure!(
            bytes.len() as u64 <= limit,
            "scoped file exceeds output limit"
        );
        let content = String::from_utf8(bytes).context("scoped file is not UTF-8")?;
        let output = serde_json::json!({"path": relative, "content": content});
        ensure!(
            serde_json::to_vec(&output)?.len() as u64
                <= self
                    .binding
                    .restrictions
                    .max_output_bytes
                    .unwrap_or(MAX_OUTPUT_BYTES),
            "serialized tool result exceeds output limit"
        );
        Ok(output)
    }
}

impl BindingRestrictions {
    fn kinds(&self) -> impl Iterator<Item = RestrictionKind> {
        let mut kinds = Vec::with_capacity(3);
        if !self.paths.is_empty() {
            kinds.push(RestrictionKind::Paths);
        }
        if !self.sources.is_empty() {
            kinds.push(RestrictionKind::Sources);
        }
        if self.max_output_bytes.is_some() {
            kinds.push(RestrictionKind::MaxOutputBytes);
        }
        kinds.into_iter()
    }
}

fn toml_table_to_json(table: &toml::Table) -> Result<Value> {
    serde_json::to_value(table).context("serializing tool binding table")
}

fn schema_properties(schema: &Value) -> Result<&serde_json::Map<String, Value>> {
    schema
        .get("properties")
        .and_then(Value::as_object)
        .context("schema properties must be an object")
}

fn validate_fixed(schema: &Value, fixed: &Value) -> Result<()> {
    let properties = schema_properties(schema)?;
    let fixed = fixed
        .as_object()
        .context("fixed arguments must be an object")?;
    for (name, value) in fixed {
        let property = properties
            .get(name)
            .with_context(|| format!("fixed argument '{name}' is not in the input schema"))?;
        validate_schema_value(property, value, &format!("fixed.{name}"))?;
    }
    Ok(())
}

fn derive_public_schema(schema: &Value, fixed: &toml::Table) -> Result<Value> {
    let mut public = schema.clone();
    let object = public
        .as_object_mut()
        .context("input schema must be an object")?;
    let properties = object
        .get_mut("properties")
        .and_then(Value::as_object_mut)
        .context("input schema properties must be an object")?;
    for name in fixed.keys() {
        ensure!(
            properties.remove(name).is_some(),
            "fixed argument '{name}' is not in the input schema"
        );
    }
    if let Some(required) = object.get_mut("required").and_then(Value::as_array_mut) {
        required.retain(|name| !name.as_str().is_some_and(|name| fixed.contains_key(name)));
    }
    Ok(public)
}

/// Validate the strict schema subset used by built-in descriptors. Factories
/// remain responsible for semantic checks and any additional Draft 2020-12 keywords.
fn validate_schema_value(schema: &Value, value: &Value, label: &str) -> Result<()> {
    if is_secret_reference(value) {
        ensure!(
            schema.get("x-secret").and_then(Value::as_bool) == Some(true),
            "{label} does not permit a secret reference"
        );
        return Ok(());
    }
    if let Some(expected) = schema.get("type").and_then(Value::as_str) {
        let matches = match expected {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
            "number" => value.is_number(),
            "boolean" => value.is_boolean(),
            "null" => value.is_null(),
            _ => bail!("{label} uses unsupported schema type '{expected}'"),
        };
        ensure!(matches, "{label} does not match schema type '{expected}'");
    }
    if let Some(choices) = schema.get("enum").and_then(Value::as_array) {
        ensure!(choices.contains(value), "{label} is not an allowed value");
    }
    if let Some(number) = value.as_f64() {
        if let Some(minimum) = schema.get("minimum").and_then(Value::as_f64) {
            ensure!(number >= minimum, "{label} is below its minimum");
        }
        if let Some(maximum) = schema.get("maximum").and_then(Value::as_f64) {
            ensure!(number <= maximum, "{label} exceeds its maximum");
        }
    }
    if let Some(string) = value.as_str() {
        let length = string.chars().count() as u64;
        if let Some(minimum) = schema.get("minLength").and_then(Value::as_u64) {
            ensure!(length >= minimum, "{label} is shorter than its minimum");
        }
        if let Some(maximum) = schema.get("maxLength").and_then(Value::as_u64) {
            ensure!(length <= maximum, "{label} exceeds its maximum length");
        }
    }
    if let Some(object) = value.as_object() {
        let properties = schema
            .get("properties")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        if schema.get("additionalProperties").and_then(Value::as_bool) == Some(false) {
            ensure!(
                object.keys().all(|name| properties.contains_key(name)),
                "{label} contains an unknown field"
            );
        }
        if let Some(required) = schema.get("required").and_then(Value::as_array) {
            for name in required.iter().filter_map(Value::as_str) {
                ensure!(object.contains_key(name), "{label}.{name} is required");
            }
        }
        for (name, item) in object {
            if let Some(property) = properties.get(name) {
                validate_schema_value(property, item, &format!("{label}.{name}"))?;
            }
        }
    }
    Ok(())
}

fn is_secret_reference(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    object.len() == 1
        && object
            .get("env")
            .and_then(Value::as_str)
            .is_some_and(valid_environment_name)
}

fn valid_environment_name(value: &str) -> bool {
    let mut chars = value.chars();
    chars
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
        && chars.all(|character| character.is_ascii_alphanumeric() || character == '_')
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
