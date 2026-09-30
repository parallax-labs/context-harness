//! Deterministic model fixture. Each call consumes one scripted outcome.
use super::*;
use std::collections::VecDeque;
use std::sync::Mutex;

pub struct FakeModel {
    outcomes: Mutex<VecDeque<ModelResult<ModelResponse>>>,
}
impl FakeModel {
    pub fn new(outcomes: impl IntoIterator<Item = ModelResult<ModelResponse>>) -> Self {
        Self {
            outcomes: Mutex::new(outcomes.into_iter().collect()),
        }
    }
}
#[async_trait]
impl ModelProvider for FakeModel {
    async fn generate(&self, request: &ModelRequest) -> ModelResult<ModelResponse> {
        request.validate()?;
        let response = self
            .outcomes
            .lock()
            .map_err(|_| ModelError::new(ModelErrorKind::ProviderFailure))?
            .pop_front()
            .ok_or_else(|| ModelError::new(ModelErrorKind::ScriptExhausted))??;
        response.validate(request)?;
        Ok(response)
    }
}
