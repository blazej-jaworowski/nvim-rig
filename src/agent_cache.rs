use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
};

use rig::{agent::Agent, client::CompletionClient, providers::openrouter};
use strum::EnumString;
use tokio::sync::Mutex;

use crate::{Error, Result, completion::Completion};

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy, strum::Display, EnumString)]
pub enum AgentModel {
    #[strum(serialize = "google/gemini-2.5-flash")]
    GeminiFast,

    #[strum(serialize = "google/gemini-3-pro-preview")]
    GeminiSmart,

    #[strum(serialize = "anthropic/claude-opus-4.5")]
    ClaudeOpus,
}

struct AgentFactory {
    client: openrouter::Client,
}

impl AgentFactory {
    pub fn new(api_key: &str) -> Self {
        Self {
            client: openrouter::Client::new(api_key),
        }
    }

    pub fn create_agent(&self, model: AgentModel) -> Agent<openrouter::CompletionModel> {
        self.client.agent(&model.to_string()).build()
    }
}

pub struct AgentCache {
    factory: AgentFactory,
    agents: Mutex<HashMap<AgentModel, Arc<Agent<openrouter::CompletionModel>>>>,
}

impl AgentCache {
    pub fn new(api_key: &str) -> Self {
        Self {
            factory: AgentFactory::new(api_key),
            agents: Mutex::new(HashMap::new()),
        }
    }

    pub async fn get_model(&self, model: AgentModel) -> Completion {
        let mut agents_guard = self.agents.lock().await;

        let agent = agents_guard
            .entry(model)
            .or_insert_with(|| Arc::new(self.factory.create_agent(model)))
            .clone();

        Completion::new(agent)
    }
}

static AGENT_CACHE: OnceLock<AgentCache> = OnceLock::new();

pub fn init_static(api_key: &str) {
    _ = AGENT_CACHE.set(AgentCache::new(api_key));
}

pub async fn get_agent(model: AgentModel) -> Result<Completion> {
    let cache = AGENT_CACHE.get().ok_or(Error::Uninitialized)?;

    Ok(cache.get_model(model).await)
}
