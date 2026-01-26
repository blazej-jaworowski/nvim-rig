use std::{collections::HashMap, sync::Arc};

use parking_lot::RwLock;
use rig::{client::CompletionClient, providers::openrouter::Client};
use tokio::runtime::Runtime;

use crate::completion::Completion;

lazy_static::lazy_static! {
    static ref AVAILABLE_MODELS: HashMap<&'static str, &'static str> =  {
        HashMap::from([
            ("GeminiFlash", "google/gemini-3-flash-preview"),
            ("GeminiPro", "google/gemini-3-pro-preview"),
            ("ClaudeOpus", "anthropic/claude-opus-4.5"),
        ])
    };
}

struct CompletionFactory {
    client: Client,
    runtime: Arc<Runtime>,
}

impl CompletionFactory {
    pub fn new(api_key: &str, runtime: Arc<Runtime>) -> Self {
        Self {
            client: Client::new(api_key),
            runtime,
        }
    }

    pub fn create_completion(&self, model: &str) -> Completion {
        // Rig agent builder requires to be in tokio context
        let agent = self
            .runtime
            .block_on(async { self.client.agent(model).build() });

        Completion::new(agent, self.runtime.clone())
    }
}

pub struct CompletionCache {
    factory: CompletionFactory,
    agents: RwLock<HashMap<String, Arc<Completion>>>,
}

impl CompletionCache {
    pub fn new(api_key: &str, runtime: Arc<Runtime>) -> Self {
        Self {
            factory: CompletionFactory::new(api_key, runtime),
            agents: RwLock::new(HashMap::new()),
        }
    }

    pub fn get_model(&self, model: &str) -> Arc<Completion> {
        {
            let agents_guard = self.agents.read();

            if let Some(agent) = agents_guard.get(model) {
                return agent.clone();
            }
        }

        let mut agents_guard = self.agents.write();

        agents_guard
            .entry(model.into())
            .or_insert_with(|| Arc::new(self.factory.create_completion(model)))
            .clone()
    }
}
