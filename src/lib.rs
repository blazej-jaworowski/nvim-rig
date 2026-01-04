use std::sync::{Arc, OnceLock};

use eel_nvim::editor::NvimEditor;
use tracing::debug;

use crate::{
    agent_cache::{AgentCache, AgentModel},
    api_key::get_api_key,
};
use eel::{Editor, cursor::CursorBuffer};

mod error;

mod agent_cache;
mod api_key;
mod completion;
mod completion_buffer;

pub use completion_buffer::CompletionBuffer;

type Error = error::Error;
type Result<T> = std::result::Result<T, Error>;

struct Plugin<E>
where
    E: Editor,
    E::Buffer: CursorBuffer,
{
    editor: Arc<E>,
    agent_cache: Arc<AgentCache>,
}

impl<E> Plugin<E>
where
    E: Editor,
    E::Buffer: CursorBuffer,
{
    fn new(editor: Arc<E>, api_key: &str) -> Self {
        Self {
            editor,
            agent_cache: Arc::new(AgentCache::new(api_key)),
        }
    }
}

static PLUGIN: OnceLock<Plugin<NvimEditor>> = OnceLock::new();

fn get_instance() -> Result<&'static Plugin<NvimEditor>> {
    PLUGIN.get().ok_or(error::Error::Uninitialized)
}

pub fn setup_rig(editor: Arc<NvimEditor>, api_key_location: &str) -> Result<()> {
    let api_key = get_api_key(api_key_location)?;

    debug!("Initializing nvim-rig");

    _ = PLUGIN
        .set(Plugin::new(editor, &api_key))
        .inspect_err(|_| tracing::warn!("Rig setup called more than once"));

    Ok(())
}

pub async fn setup_prompt_buffer() -> Result<()> {
    CompletionBuffer::<NvimEditor>::create_new(
        get_instance()?.editor.clone(),
        get_instance()?.agent_cache.clone(),
    )
    .await?;

    Ok(())
}

pub async fn prompt_buffer() -> Result<()> {
    let buffer = CompletionBuffer::<NvimEditor>::create_from(
        get_instance()?.editor.current_buffer().await?,
        get_instance()?.agent_cache.clone(),
    );

    buffer.perform_prompt(AgentModel::GeminiSmart).await?;

    Ok(())
}
