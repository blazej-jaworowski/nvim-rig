use std::sync::Arc;

use eel::{CompleteBufferHandle, Editor};

use crate::{
    CompletionBuffer, Result, completion_buffer::CompletionBufferConfig,
    completion_cache::CompletionCache,
};

pub trait ApiKeyGetter: Send + Sync {
    fn api_key(&self) -> anyhow::Result<String>;
}

impl ApiKeyGetter for String {
    fn api_key(&self) -> anyhow::Result<String> {
        Ok(self.clone())
    }
}

impl<F> ApiKeyGetter for F
where
    F: Fn() -> anyhow::Result<String>,
    F: Sync + Send,
{
    fn api_key(&self) -> anyhow::Result<String> {
        self()
    }
}

pub struct Plugin<E>
where
    E: Editor,
    E::BufferHandle: CompleteBufferHandle,
{
    editor: Arc<E>,
    completion_cache: Arc<CompletionCache>,
}

impl<E> Plugin<E>
where
    E: Editor,
    E::BufferHandle: CompleteBufferHandle,
{
    pub fn new(editor: Arc<E>, api_key_getter: impl ApiKeyGetter + 'static) -> Result<Self> {
        let runtime = tokio::runtime::Runtime::new()?;
        let cache = CompletionCache::new(api_key_getter, Arc::new(runtime))?;

        Ok(Self {
            editor,
            completion_cache: Arc::new(cache),
        })
    }

    pub fn rig_buffer(&self) -> Result<()> {
        CompletionBuffer::<E>::create_new(self.editor.clone(), self.completion_cache.clone())?;

        Ok(())
    }

    pub fn prompt_buffer(&self) -> Result<()> {
        let buffer = CompletionBuffer::<E>::create_from(
            self.editor.current_buffer()?,
            self.completion_cache.clone(),
            CompletionBufferConfig::default(),
        );

        buffer.perform_prompt()?;

        Ok(())
    }
}

pub trait StaticRig<E>
where
    E: Editor,
    E::BufferHandle: CompleteBufferHandle,
{
    fn get_instance() -> Result<&'static Plugin<E>>;
    fn setup(editor: Arc<E>, api_key_getter: impl ApiKeyGetter + 'static) -> Result<()>;

    fn rig_buffer() -> Result<()> {
        Self::get_instance()?.rig_buffer()
    }

    fn prompt_buffer() -> Result<()> {
        Self::get_instance()?.prompt_buffer()
    }
}
