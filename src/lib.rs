use crate::{agent_cache::AgentModel, api_key::get_api_key};
use nvim_api_helper::{buffer::Buffer, nvim::NvimBuffer};
use tracing::debug;

mod error;

mod agent_cache;
mod api_key;
mod completion;
mod completion_buffer;

pub use completion_buffer::CompletionBuffer;

type Error = error::Error;
type Result<T> = std::result::Result<T, Error>;

pub fn setup_rig(api_key_location: &str) -> Result<()> {
    let api_key = get_api_key(api_key_location)?;

    debug!("Initializing nvim-rig");
    agent_cache::init_static(&api_key);

    Ok(())
}

pub fn setup_prompt_buffer() -> Result<()> {
    CompletionBuffer::<NvimBuffer>::create_new()?;

    Ok(())
}

pub async fn prompt_buffer() -> Result<()> {
    let buffer = CompletionBuffer::create_from(NvimBuffer::current());

    buffer.perform_prompt(AgentModel::ClaudeOpus).await?;

    Ok(())
}
