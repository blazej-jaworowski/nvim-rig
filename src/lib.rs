use std::sync::OnceLock;

use rig::message::Message;

use futures::TryStreamExt;

use tracing::{debug, instrument};

use nvim_api_helper::{
    buffer::Buffer as _,
    nvim::NvimBuffer,
};

use crate::agent_cache::{AgentCache, AgentModel};
use crate::api_key::get_api_key;
use crate::completion::{Completion, CompletionChunk};

mod error;

mod agent_cache;
mod api_key;
mod completion;

type Error = error::Error;
type Result<T> = std::result::Result<T, Error>;

static AGENT_CACHE: OnceLock<AgentCache> = OnceLock::new();

const ASSISTANT_HEADER: &str = "# ** ----- Assistant ----- **";
const USER_HEADER: &str = "# ** ------- User -------- **";

pub fn setup_rig(api_key_location: &str) -> Result<()> {
    let api_key = get_api_key(api_key_location)?;

    debug!("Initializing nvim-rig");

    _ = AGENT_CACHE.set(AgentCache::new(&api_key));

    Ok(())
}

async fn get_agent(model: AgentModel) -> Result<Completion> {
    let cache = AGENT_CACHE.get().ok_or(Error::Uninitialized)?;

    Ok(cache.get_model(model).await)
}

fn parse_content(content: String) -> (String, Vec<Message>) {
    // Valid content should start with USER_HEADER
    if !matches!(content.lines().next(), Some(USER_HEADER)) {
        return (content, Vec::new());
    }

    let mut messages: Vec<Message> = Vec::new();

    let mut is_user_message = true;
    let mut partial_msg = String::new();

    for line in content.lines() {
        match line {
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
        (partial_msg, messages)
    } else {
        messages.push(Message::assistant(partial_msg));
        (String::new(), messages)
    }
}

#[instrument(level = "trace")]
pub async fn prompt_buffer() -> Result<()> {
    let model = AgentModel::GeminiSmart;
    let content = nvim_api_helper::nvim_dispatch! {
        let buf = NvimBuffer::current();

        buf.get_content()
    }
    .await??;

    let agent = get_agent(model).await?;

    let (prompt, messages) = parse_content(content);

    let mut stream = agent.stream_chat(&prompt, messages).await;

    nvim_api_helper::nvim_dispatch! {
        let mut buf = NvimBuffer::current();
        buf.append(&format!("\n\n{ASSISTANT_HEADER}\n\n"))
    }
    .await??;

    while let Some(chunk) = stream.try_next().await? {
        match chunk {
            CompletionChunk::Text(text) => {
                nvim_api_helper::nvim_dispatch! {
                    let mut buf = NvimBuffer::current();
                    buf.append(&text)
                }
                .await??;
            }
        }
    }

    nvim_api_helper::nvim_dispatch! {
        let mut buf = NvimBuffer::current();
        buf.append(&format!("\n\n{USER_HEADER}\n\n"))
    }
    .await??;

    Ok(())
}

pub fn setup_prompt_buffer() -> nvim_api_helper::Result<()> {
    _ = nvim_api_helper::nvim::nvim_oxi::api::command("edit /tmp/conversation.md");

    let mut buf = NvimBuffer::current();
    buf.set_content(&format!("{USER_HEADER}\n\n"))?;

    let (row, col) = buf.max_pos()?;
    buf.set_cursor(row, col)?;

    Ok(())
}
