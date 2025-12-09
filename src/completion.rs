use std::{pin::Pin, sync::Arc};

use async_stream::stream;
use futures::{Stream, StreamExt};
use rig::{
    agent::{Agent, MultiTurnStreamItem},
    message::Message,
    providers::openrouter,
    streaming::{StreamedAssistantContent, StreamingChat},
};

pub struct Completion {
    agent: Arc<Agent<openrouter::CompletionModel>>,
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
        Self { agent }
    }

    #[allow(dead_code)]
    pub async fn stream_prompt(
        &self,
        prompt: &str,
    ) -> Pin<Box<dyn Stream<Item = Result<CompletionChunk>> + Send>> {
        self.stream_chat(prompt, Vec::new()).await
    }

    pub async fn stream_chat(
        &self,
        prompt: &str,
        chat_history: Vec<Message>,
    ) -> Pin<Box<dyn Stream<Item = Result<CompletionChunk>> + Send>> {
        let mut stream = self.agent.stream_chat(prompt, chat_history).await;

        let out_stream = stream! {
            while let Some(chunk) = stream.next().await {
                let chunk = match chunk {
                    Ok(c) => c,
                    Err(e) => {
                        yield Err(Error::Rig(e.to_string()));
                        continue;
                    },
                };

                let assistant_item = match chunk {
                    MultiTurnStreamItem::StreamAssistantItem(i) => i,
                    _ => continue,
                };

                match assistant_item {
                    StreamedAssistantContent::Text(content) => {
                        yield Ok(CompletionChunk::Text(content.text().into()));
                        continue;
                    },
                    StreamedAssistantContent::Reasoning(_) => {
                        // Ignore for now, openrouter doesn't seem to support reasoning tokens
                        continue;
                    },
                    _ => continue,
                };
            }
        };

        Box::pin(out_stream)
    }
}
