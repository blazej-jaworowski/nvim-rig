use itertools::Itertools as _;
use rig::message::Message;

use futures::TryStreamExt;

use tracing::instrument;

use crate::{
    Result,
    agent_cache::{AgentModel, get_agent},
    completion::CompletionChunk,
};
use nvim_api_helper::buffer::Buffer;

const ASSISTANT_HEADER: &str = "# ** ----- Assistant ----- **";
const USER_HEADER: &str = "# ** ------- User -------- **";

#[derive(derivative::Derivative)]
#[derivative(Debug)]
pub struct CompletionBuffer<B: Buffer> {
    #[derivative(Debug = "ignore")]
    inner: B,
}

impl<B: Buffer> CompletionBuffer<B> {
    fn parse_content(&self) -> Result<(String, Vec<Message>)> {
        let mut lines = self.inner.get_all_lines()?.peekable();

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

    pub fn create_new() -> Result<Self> {
        let buf = B::create_new("markdown")?;
        buf.set_content(&format!("{USER_HEADER}\n\n"))?;

        buf.set_current()?;

        let (row, col) = buf.max_pos()?;
        buf.set_cursor(row, col)?;

        Ok(Self::create_from(buf))
    }

    pub fn create_from(buffer: B) -> Self {
        Self { inner: buffer }
    }

    #[instrument(level = "trace")]
    pub async fn perform_prompt(&self, model: AgentModel) -> Result<()> {
        let agent = get_agent(model).await?;
        let (prompt, messages) = self.parse_content()?;

        let mut stream = agent.stream_chat(&prompt, messages).await;

        self.inner.append(&format!("\n\n{ASSISTANT_HEADER}\n\n"))?;

        while let Some(chunk) = stream.try_next().await? {
            match chunk {
                CompletionChunk::Text(text) => {
                    self.inner.append(&text)?;
                }
            }
        }

        self.inner.append(&format!("\n\n{USER_HEADER}\n\n"))?;

        Ok(())
    }
}
