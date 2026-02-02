use std::{collections::HashMap, sync::Arc};

use parking_lot::RwLock;
use rig::{client::CompletionClient, providers::openrouter::Client};
use tokio::runtime::Runtime;

use crate::{Result, completion::Completion, plugin::ApiKeyGetter};

struct CompletionFactory {
    client: Client,
    runtime: Arc<Runtime>,
}

impl CompletionFactory {
    pub fn new(api_key_getter: impl ApiKeyGetter, runtime: Arc<Runtime>) -> Result<Self> {
        let api_key = api_key_getter
            .api_key()
            .map_err(crate::error::Error::ApiKey)?;

        Ok(Self {
            client: Client::new(api_key)?,
            runtime,
        })
    }

    pub fn create_completion(&self, model: &str) -> Completion {
        // Rig agent builder requires to be in tokio context
        let agent = self.runtime.block_on(async {
            self.client
                .agent(model)
                .preamble(
                    r#"
### Role and Persona
You are a precise and structured AI assistant. Your goal is to provide clear, accurate responses.

### Output Format Guidelines
**Markdown Structure**:
- Use Markdown for all output.
- **Never** use Heading Level 1 (#).
- Start all section headers at Heading Level 2 (##) or deeper (###, ####).
"#,
                )
                .build()
        });

        Completion::new(agent, self.runtime.clone())
    }
}

pub struct CompletionCache {
    factory: CompletionFactory,
    agents: RwLock<HashMap<String, Arc<Completion>>>,
}

impl CompletionCache {
    pub fn new(api_key_getter: impl ApiKeyGetter, runtime: Arc<Runtime>) -> Result<Self> {
        Ok(Self {
            factory: CompletionFactory::new(api_key_getter, runtime)?,
            agents: RwLock::new(HashMap::new()),
        })
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
