//! Trusted, compiled model-provider registration and construction.
//!
//! Configuration selects only factories already registered by the host. Static
//! validation never constructs a provider or resolves credentials.

use super::{fake, ollama, openai, ModelProvider, ModelResponse};
use crate::agent_resource::ModelDefinition;
use anyhow::{ensure, Context};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Stable provider implementation metadata for compatibility decisions.
///
/// The identifier names the adapter implementation, not a configured provider
/// alias or model. The version changes when adapter behavior becomes
/// checkpoint-incompatible. Neither value may contain credentials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelProviderImplementation {
    id: String,
    version: String,
}

impl ModelProviderImplementation {
    pub fn new(id: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            version: version.into(),
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    fn validate(&self) -> anyhow::Result<()> {
        ensure!(
            !self.id.trim().is_empty() && !self.version.trim().is_empty(),
            "model provider implementation identity is required"
        );
        Ok(())
    }
}

/// A compiled, trusted provider factory registered explicitly by the host.
pub trait ModelProviderFactory: Send + Sync {
    /// Configuration name used by `ModelDefinition::provider`.
    fn provider_name(&self) -> &str;

    /// Stable, non-secret adapter identity and compatibility version.
    fn implementation(&self) -> ModelProviderImplementation;

    /// Validate provider-specific configuration without side effects.
    fn validate(&self, definition: &ModelDefinition) -> anyhow::Result<()>;

    /// Construct the provider for runtime execution.
    fn build(&self, definition: &ModelDefinition) -> anyhow::Result<Arc<dyn ModelProvider>>;
}

#[derive(Clone)]
struct RegisteredFactory {
    implementation: ModelProviderImplementation,
    factory: Arc<dyn ModelProviderFactory>,
}

/// Catalog of trusted model-provider factories.
///
/// Provider names are case-sensitive and first-registration wins: registering
/// an existing name returns an error and leaves the catalog unchanged.
#[derive(Clone, Default)]
pub struct ModelProviderCatalog {
    factories: BTreeMap<String, RegisteredFactory>,
}

impl ModelProviderCatalog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_builtins() -> anyhow::Result<Self> {
        let mut catalog = Self::new();
        catalog.register(Arc::new(OpenAiFactory))?;
        catalog.register(Arc::new(FakeFactory))?;
        catalog.register(Arc::new(OllamaFactory))?;
        Ok(catalog)
    }

    pub fn register(&mut self, factory: Arc<dyn ModelProviderFactory>) -> anyhow::Result<()> {
        let name = factory.provider_name();
        ensure!(!name.trim().is_empty(), "model provider name is required");
        ensure!(
            !self.factories.contains_key(name),
            "model provider '{name}' is already registered"
        );
        let implementation = factory.implementation();
        implementation.validate()?;
        self.factories.insert(
            name.to_owned(),
            RegisteredFactory {
                implementation,
                factory,
            },
        );
        Ok(())
    }

    pub fn implementation(
        &self,
        provider_name: &str,
    ) -> anyhow::Result<&ModelProviderImplementation> {
        Ok(&self
            .factories
            .get(provider_name)
            .with_context(|| format!("unsupported model provider '{provider_name}'"))?
            .implementation)
    }

    /// Validate definitions without constructing providers or resolving secrets.
    pub fn validate_config(
        &self,
        models: &BTreeMap<String, ModelDefinition>,
    ) -> anyhow::Result<()> {
        for (alias, definition) in models {
            self.validate_definition(alias, definition)?;
        }
        Ok(())
    }

    pub(crate) fn validate_definition(
        &self,
        alias: &str,
        definition: &ModelDefinition,
    ) -> anyhow::Result<()> {
        definition
            .validate()
            .with_context(|| format!("model '{alias}'"))?;
        let registered = self.factories.get(&definition.provider).with_context(|| {
            format!(
                "unsupported model provider '{}' for '{alias}'",
                definition.provider
            )
        })?;
        registered
            .factory
            .validate(definition)
            .with_context(|| format!("model '{alias}'"))
    }

    pub(crate) fn build(
        &self,
        alias: &str,
        definition: &ModelDefinition,
    ) -> anyhow::Result<(Arc<dyn ModelProvider>, ModelProviderImplementation)> {
        let registered = self.factories.get(&definition.provider).with_context(|| {
            format!(
                "unsupported model provider '{}' for '{alias}'",
                definition.provider
            )
        })?;
        let provider = registered
            .factory
            .build(definition)
            .with_context(|| format!("model '{alias}'"))?;
        Ok((provider, registered.implementation.clone()))
    }
}

struct OpenAiFactory;

impl ModelProviderFactory for OpenAiFactory {
    fn provider_name(&self) -> &str {
        "openai"
    }

    fn implementation(&self) -> ModelProviderImplementation {
        ModelProviderImplementation::new("context-harness.openai-responses", "1")
    }

    fn validate(&self, definition: &ModelDefinition) -> anyhow::Result<()> {
        ensure!(
            definition.base_url.is_none() && definition.timeout_seconds.is_none(),
            "base_url and timeout_seconds are only supported by the ollama provider"
        );
        Ok(())
    }

    fn build(&self, definition: &ModelDefinition) -> anyhow::Result<Arc<dyn ModelProvider>> {
        Ok(Arc::new(openai::OpenAiProvider::new(
            &definition.model,
            definition
                .api_key_env
                .as_deref()
                .unwrap_or("OPENAI_API_KEY"),
        )?))
    }
}

struct FakeFactory;

impl ModelProviderFactory for FakeFactory {
    fn provider_name(&self) -> &str {
        "fake"
    }

    fn implementation(&self) -> ModelProviderImplementation {
        ModelProviderImplementation::new("context-harness.fake", "1")
    }

    fn validate(&self, definition: &ModelDefinition) -> anyhow::Result<()> {
        ensure!(
            definition.base_url.is_none() && definition.timeout_seconds.is_none(),
            "base_url and timeout_seconds are only supported by the ollama provider"
        );
        Ok(())
    }

    fn build(&self, _definition: &ModelDefinition) -> anyhow::Result<Arc<dyn ModelProvider>> {
        Ok(Arc::new(fake::FakeModel::new([Ok(ModelResponse::text(
            "Synthetic response from FakeModel; no model service was called.",
        ))])))
    }
}

struct OllamaFactory;

impl ModelProviderFactory for OllamaFactory {
    fn provider_name(&self) -> &str {
        "ollama"
    }

    fn implementation(&self) -> ModelProviderImplementation {
        ModelProviderImplementation::new("context-harness.ollama-chat", "1")
    }

    fn validate(&self, definition: &ModelDefinition) -> anyhow::Result<()> {
        ollama::validate_definition(definition)
    }

    fn build(&self, definition: &ModelDefinition) -> anyhow::Result<Arc<dyn ModelProvider>> {
        Ok(Arc::new(ollama::OllamaProvider::from_definition(
            definition,
        )?))
    }
}
