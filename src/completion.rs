use std::sync::Arc;

use futures::StreamExt;
use rig::{
    agent::{Agent, MultiTurnStreamItem},
    message::Message,
    providers::openrouter::{CompletionModel, streaming::FinalCompletionResponse},
    streaming::{StreamedAssistantContent, StreamingChat},
};
use tokio::runtime::Runtime;

use crate::Result;

pub struct Completion {
    agent: Agent<CompletionModel>,
    runtime: Arc<Runtime>,
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

    Unsupported,
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Rig error: {0}")]
    Rig(String),
}

impl Completion {
    pub fn new(agent: Agent<CompletionModel>, runtime: Arc<Runtime>) -> Self {
        Self { agent, runtime }
    }

    #[allow(dead_code)]
    pub fn stream_prompt(&self, prompt: &str) -> impl IntoIterator<Item = Result<CompletionChunk>> {
        self.stream_chat(prompt, Vec::new())
    }

    fn map_chunk<E>(
        chunk: std::result::Result<MultiTurnStreamItem<FinalCompletionResponse>, E>,
    ) -> Result<CompletionChunk>
    where
        E: std::error::Error,
    {
        let chunk = match chunk {
            Ok(c) => c,
            Err(e) => {
                return Err(Error::Rig(e.to_string()).into());
            }
        };

        let assistant_item = match chunk {
            MultiTurnStreamItem::StreamAssistantItem(i) => i,
            _ => return Ok(CompletionChunk::Unsupported),
        };

        let mapped_chunk = match assistant_item {
            StreamedAssistantContent::Text(content) => CompletionChunk::Text(content.text().into()),
            StreamedAssistantContent::Reasoning(_) => {
                // Ignore for now, openrouter doesn't seem to support reasoning tokens
                CompletionChunk::Unsupported
            }
            _ => CompletionChunk::Unsupported,
        };

        Ok(mapped_chunk)
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
                // TODO: Handle this error
                _ = tx.send(Self::map_chunk(chunk));
            }
        });

        rx
    }
}
