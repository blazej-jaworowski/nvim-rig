use std::sync::Arc;

use itertools::Itertools as _;
use rig::message::Message;

use futures::TryStreamExt;

use tracing::instrument;

use eel::{
    Editor,
    buffer::{Buffer, BufferHandle},
    cursor::CursorBuffer,
};

use crate::{
    Result,
    agent_cache::{AgentCache, AgentModel},
    completion::CompletionChunk,
};

const ASSISTANT_HEADER: &str = "# ** ----- Assistant ----- **";
const USER_HEADER: &str = "# ** ------- User -------- **";

#[derive(derivative::Derivative)]
#[derivative(Debug)]
pub struct CompletionBuffer<E>
where
    E: Editor,
    E::Buffer: CursorBuffer,
{
    #[derivative(Debug = "ignore")]
    inner: E::BufferHandle,

    #[derivative(Debug = "ignore")]
    agent_cache: Arc<AgentCache>,
}

impl<E> CompletionBuffer<E>
where
    E: Editor,
    E::Buffer: CursorBuffer,
{
    fn parse_content(&self) -> impl Future<Output = Result<(String, Vec<Message>)>> + Send {
        let fut = self.inner.read();
        async {
            let buffer = fut.await;

            let mut lines = buffer.get_all_lines().await?.peekable();

            // Valid content should start with USER_HEADER
            if !matches!(lines.peek().map(String::as_str), Some(USER_HEADER)) {
                return Ok((lines.join("\n"), Vec::new()));
            }

            let mut messages: Vec<Message> = Vec::new();
            let mut is_user_message = true;
            let mut partial_msg = String::new();

            for line in lines {
                // TODO: Ugly
                match line.as_str() {
                    "" if partial_msg.is_empty() => continue,
                    ASSISTANT_HEADER => {
                        if !partial_msg.is_empty() {
                            let message = if is_user_message {
                                Message::user(partial_msg)
                            } else {
                                Message::assistant(partial_msg)
                            };
                            messages.push(message);

                            partial_msg = String::new();
                        }
                        is_user_message = false;
                    }
                    USER_HEADER => {
                        if !partial_msg.is_empty() {
                            let message = if is_user_message {
                                Message::user(partial_msg)
                            } else {
                                Message::assistant(partial_msg)
                            };
                            messages.push(message);

                            partial_msg = String::new();
                        }
                        is_user_message = true;
                    }
                    l => {
                        partial_msg.push_str(l);
                        partial_msg.push('\n');
                    }
                }
            }

            if is_user_message {
                Ok((partial_msg, messages))
            } else {
                messages.push(Message::assistant(partial_msg));
                Ok((String::new(), messages))
            }
        }
    }
    pub async fn create_new(editor: Arc<E>, agent_cache: Arc<AgentCache>) -> Result<Self> {
        let buf = editor.new_buffer().await?;
        {
            let mut buf = buf.write().await;

            buf.set_content(&format!("{USER_HEADER}\n\n")).await?;

            editor.set_current_buffer(&mut buf).await?;

            let pos = buf.max_pos().await?;
            buf.set_cursor(&pos).await?;
        }

        Ok(Self::create_from(buf, agent_cache))
    }

    pub fn create_from(buf_handle: E::BufferHandle, agent_cache: Arc<AgentCache>) -> Self {
        Self {
            inner: buf_handle,
            agent_cache,
        }
    }

    #[instrument(level = "trace")]
    pub async fn perform_prompt(&self, model: AgentModel) -> Result<()> {
        let agent = self.agent_cache.get_model(model).await;
        let (prompt, messages) = self.parse_content().await?;

        let mut stream = agent.stream_chat(&prompt, messages).await;

        self.inner
            .write()
            .await
            .append(&format!("\n\n{ASSISTANT_HEADER}\n\n"))
            .await?;

        while let Some(chunk) = stream.try_next().await? {
            match chunk {
                CompletionChunk::Text(text) => {
                    self.inner.write().await.append(&text).await?;
                }
            }
        }

        self.inner
            .write()
            .await
            .append(&format!("\n\n{USER_HEADER}\n\n"))
            .await?;

        Ok(())
    }
}
