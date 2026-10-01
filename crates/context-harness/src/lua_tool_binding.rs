//! Explicitly enrolled privileged Lua implementations for local agent bindings.
//! Manifests and script bytes are inspected without evaluating Lua.

use crate::{
    agent_resource::Capability,
    config::Config,
    tool_binding::{
        validate_binding_arguments, RestrictionKind, ToolBindingRequest,
        ToolImplementationDescriptor, ToolImplementationFactory, ToolTrustClass,
    },
    tool_script::{execute_tool, validate_params, ToolDefinition},
    traits::{Tool, ToolContext, ToolPreparation},
};
use anyhow::{ensure, Context, Result};
use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

const ALL_HOST_APIS: [&str; 9] = [
    "base64", "context", "crypto", "env", "fs", "http", "json", "log", "sleep",
];

#[derive(Debug, Clone, Serialize)]
pub struct LuaToolManifest {
    pub implementation_id: String,
    pub script_path: PathBuf,
    pub implementation_description: String,
    pub default_public_description: String,
    pub config_schema: Value,
    pub input_schema: Value,
    pub capabilities: Vec<Capability>,
    pub permitted_host_apis: Vec<String>,
    pub timeout_seconds: u64,
    pub max_output_bytes: u64,
}

pub struct LuaToolFactory {
    descriptor: ToolImplementationDescriptor,
    manifest: LuaToolManifest,
    script_source: String,
    config: Arc<Config>,
}

impl LuaToolFactory {
    pub fn from_manifest(mut manifest: LuaToolManifest, config: Arc<Config>) -> Result<Self> {
        ensure!(
            manifest.implementation_id.starts_with("lua."),
            "Lua implementation IDs must use the lua namespace"
        );
        ensure!(
            (1..=300).contains(&manifest.timeout_seconds),
            "Lua timeout must be in 1..=300 seconds"
        );
        ensure!(
            (1..=1024 * 1024).contains(&manifest.max_output_bytes),
            "Lua output limit must be in 1..=1048576 bytes"
        );
        manifest.permitted_host_apis.sort();
        manifest.permitted_host_apis.dedup();
        ensure!(
            manifest.permitted_host_apis == ALL_HOST_APIS,
            "version 1 privileged Lua manifests must declare every exposed host API"
        );
        for required in [
            Capability::ReadOnly,
            Capability::Network,
            Capability::ExternalSideEffect,
        ] {
            ensure!(
                manifest.capabilities.contains(&required),
                "privileged Lua manifest is missing required capability"
            );
        }
        manifest.script_path = manifest
            .script_path
            .canonicalize()
            .context("canonicalizing Lua script")?;
        let script_source = std::fs::read_to_string(&manifest.script_path)
            .context("reading enrolled Lua script")?;
        let manifest_bytes = serde_json::to_vec(&manifest)?;
        let mut digest = Sha256::new();
        digest.update(&manifest_bytes);
        digest.update(script_source.as_bytes());
        let descriptor = ToolImplementationDescriptor {
            id: manifest.implementation_id.clone(),
            version: format!("sha256:{:x}", digest.finalize()),
            implementation_description: manifest.implementation_description.clone(),
            default_public_description: manifest.default_public_description.clone(),
            config_schema: manifest.config_schema.clone(),
            input_schema: manifest.input_schema.clone(),
            capabilities: manifest.capabilities.clone(),
            supported_restrictions: vec![RestrictionKind::MaxOutputBytes],
            trust_class: ToolTrustClass::LuaPrivileged,
        };
        Ok(Self {
            descriptor,
            manifest,
            script_source,
            config,
        })
    }
}

#[async_trait]
impl ToolImplementationFactory for LuaToolFactory {
    fn descriptor(&self) -> &ToolImplementationDescriptor {
        &self.descriptor
    }

    async fn bind(&self, request: ToolBindingRequest) -> Result<Box<dyn Tool>> {
        let configured_limit = request
            .resolved
            .restrictions
            .max_output_bytes
            .unwrap_or(self.manifest.max_output_bytes)
            .min(self.manifest.max_output_bytes);
        Ok(Box::new(BoundLuaTool {
            definition: ToolDefinition {
                name: request.resolved.name.clone(),
                description: request.resolved.description.clone(),
                parameters_schema: self.descriptor.input_schema.clone(),
                script_path: self.manifest.script_path.clone(),
                script_source: self.script_source.clone(),
                config: request.resource.config.clone(),
                timeout: self.manifest.timeout_seconds,
            },
            binding: request.resolved,
            config: self.config.clone(),
            prepared: AtomicBool::new(false),
            output_limit: configured_limit,
        }))
    }
}

struct BoundLuaTool {
    definition: ToolDefinition,
    binding: crate::tool_binding::ResolvedToolBinding,
    config: Arc<Config>,
    prepared: AtomicBool,
    output_limit: u64,
}

impl BoundLuaTool {
    fn effective_arguments(&self, arguments: Value) -> Result<Value> {
        validate_binding_arguments(&self.binding.public_schema, &arguments)?;
        let mut arguments = arguments
            .as_object()
            .context("arguments must be an object")?
            .clone();
        for (name, value) in self
            .binding
            .fixed
            .as_object()
            .context("fixed arguments must be an object")?
        {
            ensure!(
                !arguments.contains_key(name),
                "fixed argument cannot be overridden"
            );
            arguments.insert(name.clone(), value.clone());
        }
        let arguments = Value::Object(arguments);
        validate_binding_arguments(&self.definition.parameters_schema, &arguments)?;
        validate_params(&self.definition.parameters_schema, &arguments)
    }
}

#[async_trait]
impl Tool for BoundLuaTool {
    fn capabilities(&self) -> Option<Vec<Capability>> {
        Some(self.binding.capabilities.clone())
    }
    fn binding_metadata(&self) -> Option<Value> {
        Some(serde_json::json!({
            "binding_version": self.binding.binding_version,
            "implementation_id": self.binding.implementation_id,
            "implementation_version": self.binding.implementation_version,
            "trust_class": self.binding.trust_class,
        }))
    }
    fn preparation(&self) -> Option<ToolPreparation> {
        Some(ToolPreparation {
            name: format!("runtime.lua.prepare.{}", self.binding.implementation_id),
            capabilities: self.binding.capabilities.clone(),
            arguments: serde_json::json!({
                "binding": self.binding.name,
                "implementation": self.binding.implementation_id,
                "version": self.binding.implementation_version,
            }),
        })
    }
    async fn prepare(&self, _ctx: &ToolContext) -> Result<()> {
        self.prepared.store(true, Ordering::SeqCst);
        Ok(())
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
        validate_binding_arguments(&self.binding.public_schema, arguments)
    }
    fn approval_arguments(&self, arguments: &Value) -> Result<Value> {
        self.effective_arguments(arguments.clone())
    }
    async fn execute(&self, arguments: Value, _ctx: &ToolContext) -> Result<Value> {
        ensure!(
            self.prepared.load(Ordering::SeqCst),
            "Lua tool was not prepared"
        );
        let result = execute_tool(
            &self.definition,
            self.effective_arguments(arguments)?,
            &self.config,
        )
        .await?;
        ensure!(
            serde_json::to_vec(&result)?.len() as u64 <= self.output_limit,
            "Lua tool result exceeds output limit"
        );
        Ok(result)
    }
}
