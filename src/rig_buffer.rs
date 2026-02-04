use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use itertools::Itertools;
use tracing::instrument;

use eel::{
    CompleteBufferHandle, Editor,
    buffer::{BufferHandle, ReadBuffer as _, WriteBuffer},
    cursor::CursorWriteBuffer,
    mark::Mark,
    region::BufferRegion,
};

use crate::{
    Result,
    completion::CompletionChunk,
    completion_cache::{CompletionCache, CompletionConfig},
    rig_buffer_format::RigBufferFormat,
};

#[derive(derivative::Derivative)]
#[derivative(Debug)]
pub struct RigBuffer<E>
where
    E: Editor,
{
    #[derivative(Debug = "ignore")]
    inner: E::BufferHandle,

    #[derivative(Debug = "ignore")]
    completion_cache: Arc<CompletionCache>,

    format: RigBufferFormat,
}

impl<E> RigBuffer<E>
where
    E: Editor,
    E::BufferHandle: CompleteBufferHandle,
{
    pub fn create_new(editor: Arc<E>, completion_cache: Arc<CompletionCache>) -> Result<Self> {
        let format = RigBufferFormat::default();
        let buffer = editor.new_buffer()?;
        {
            let mut lock = buffer.write();

            editor.set_current_buffer(&mut lock)?;

            lock.set_content(&format.default_buffer_content())?;

            let pos = lock.max_pos()?;
            lock.set_cursor(&pos)?;
        }

        Ok(Self::create_from(buffer, completion_cache, format))
    }

    pub fn create_from(
        buffer: E::BufferHandle,
        completion_cache: Arc<CompletionCache>,
        format: RigBufferFormat,
    ) -> Self {
        Self {
            inner: buffer,
            completion_cache,
            format,
        }
    }

    fn run_indicator(
        finished: Arc<AtomicBool>,
        status_region: BufferRegion<E::BufferHandle>,
    ) -> Result<()> {
        let mut indicator = ["/", "-", "\\", "|"].into_iter().cycle();

        let mut lock = status_region.write();

        lock.set_content("\nGenerating... ")?;

        let max_pos = lock.max_pos()?;
        let indicator_region = BufferRegion::new(&status_region, &max_pos, &max_pos, lock)?;

        while !finished.load(Ordering::Relaxed) {
            let mut lock = indicator_region.write();

            lock.set_content(indicator.next().unwrap())?;

            drop(lock);

            std::thread::sleep(Duration::from_millis(100));
        }

        status_region.write().set_content("")?;

        Ok::<_, crate::Error>(())
    }

    fn insert_response(
        response_stream: impl Iterator<Item = Result<CompletionChunk>>,
        insert_mark: Mark<E::BufferHandle>,
    ) -> Result<()> {
        for chunk in response_stream {
            match chunk? {
                CompletionChunk::Text(text) => {
                    insert_mark.lock_write().append_at(&text)?;
                }
                CompletionChunk::Unsupported => {}
            }
        }

        Ok(())
    }

    #[instrument(level = "trace")]
    pub fn perform_prompt(&self) -> Result<()> {
        let info = self
            .format
            .parse_content(&self.inner.read().get_all_lines()?.join("\n"));

        let agent = self.completion_cache.get_completion(&CompletionConfig::new(
            info.completion_config.model,
            info.completion_config.preamble,
        ));

        let stream = agent.stream_chat(info.messages).into_iter();

        let mut lock = self.inner.write();

        lock.append("\n\n")?;
        lock.append(&self.format.assisstant_header())?;
        lock.append("\n\n\n")?;

        let max_pos = lock.max_pos()?;
        let insert_mark = Mark::new(&self.inner, &max_pos.clone().prev_row(), &mut *lock)?;

        let finished = Arc::new(AtomicBool::new(false));
        let status_region = BufferRegion::new(&self.inner, &max_pos, &max_pos, &mut *lock)?;

        drop(lock);

        let status_handle = {
            let finished = finished.clone();
            std::thread::spawn(move || Self::run_indicator(finished, status_region))
        };

        let insert_result = Self::insert_response(stream, insert_mark);

        finished.store(true, Ordering::Relaxed);
        status_handle
            .join()
            .expect("Failed to join status thread")?;

        if insert_result.is_err() {
            self.inner.write().append("\n\nInference failed\n\n")?;
        }

        let mut lock = self.inner.write();

        lock.append("\n\n")?;
        lock.append(&self.format.user_header())?;
        lock.append("\n\n")?;

        insert_result
    }
}
