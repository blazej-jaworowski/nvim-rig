use std::sync::Arc;

use futures::StreamExt;
use rig::{
    agent::{Agent, MultiTurnStreamItem},
    message::Message,
    providers::openrouter,
    streaming::{StreamedAssistantContent, StreamingChat},
};

pub struct Completion {
    agent: Arc<Agent<openrouter::CompletionModel>>,
    runtime: tokio::runtime::Runtime,
}

impl std::fmt::Debug for Completion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Completion")
            .field("agent", &self.agent.name)
            .finish()
    }
}

#[derive(Debug)]
pub enum CompletionChunk {
    Text(String),
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Rig error: {0}")]
    Rig(String),
}

type Result<T> = std::result::Result<T, Error>;

impl Completion {
    pub fn new(agent: Arc<Agent<openrouter::CompletionModel>>) -> Self {
        Self {
            agent,
            runtime: tokio::runtime::Runtime::new().expect("Failed to create runtime"),
        }
    }

    #[allow(dead_code)]
    pub fn stream_prompt(&self, prompt: &str) -> impl IntoIterator<Item = Result<CompletionChunk>> {
        self.stream_chat(prompt, Vec::new())
    }

    pub fn stream_chat(
        &self,
        prompt: &str,
        chat_history: Vec<Message>,
    ) -> impl IntoIterator<Item = Result<CompletionChunk>> {
        let stream = self.agent.stream_chat(prompt, chat_history);

        let (tx, rx) = std::sync::mpsc::channel();

        self.runtime.spawn(async move {
            let mut stream = stream.await;
            while let Some(chunk) = stream.next().await {
                let chunk = match chunk {
                    Ok(c) => c,
                    Err(e) => {
                        _ = tx.send(Err(Error::Rig(e.to_string())));
                        continue;
                    }
                };

                let assistant_item = match chunk {
                    MultiTurnStreamItem::StreamAssistantItem(i) => i,
                    _ => continue,
                };

                match assistant_item {
                    StreamedAssistantContent::Text(content) => {
                        _ = tx.send(Ok(CompletionChunk::Text(content.text().into())));
                        continue;
                    }
                    StreamedAssistantContent::Reasoning(_) => {
                        // Ignore for now, openrouter doesn't seem to support reasoning tokens
                        continue;
                    }
                    _ => continue,
                };
            }
        });

        rx
    }
}
