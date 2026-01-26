use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use itertools::Itertools as _;
use rig::message::Message;

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
{
    #[derivative(Debug = "ignore")]
    inner: E::BufferHandle,

    #[derivative(Debug = "ignore")]
    agent_cache: Arc<AgentCache>,
}

impl<E> CompletionBuffer<E>
where
    E: Editor,
    E::BufferHandle: CompleteBufferHandle,
{
    fn parse_content(&self) -> Result<(String, Vec<Message>)> {
        let buffer = self.inner.read();

        let mut lines = buffer.get_all_lines()?.peekable();

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
    pub fn create_new(editor: Arc<E>, agent_cache: Arc<AgentCache>) -> Result<Self> {
        let buf = editor.new_buffer()?;
        {
            let mut buf = buf.write();

            buf.set_content(&format!("{USER_HEADER}\n\n"))?;

            editor.set_current_buffer(&mut buf)?;

            let pos = buf.max_pos()?;
            buf.set_cursor(&pos)?;
        }

        Ok(Self::create_from(buf, agent_cache))
    }

    pub fn create_from(buf_handle: E::BufferHandle, agent_cache: Arc<AgentCache>) -> Self {
        Self {
            inner: buf_handle,
            agent_cache,
        }
    }

    pub fn run_indicator(
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

    #[instrument(level = "trace")]
    pub fn perform_prompt(&self, model: AgentModel) -> Result<()> {
        let agent = self.agent_cache.get_model(model);
        let (prompt, messages) = self.parse_content()?;

        let stream = agent.stream_chat(&prompt, messages).into_iter();

        let mut lock = self.inner.write();

        lock.append(&format!("\n\n{ASSISTANT_HEADER}\n\n\n"))?;

        let max_pos = lock.max_pos()?;
        let insert_mark = Mark::new(&self.inner, &max_pos.clone().prev_row(), &mut *lock)?;

        let finished = Arc::new(AtomicBool::new(false));
        let status_region = BufferRegion::new(&self.inner, &max_pos, &max_pos, &mut *lock)?;

        drop(lock);

        let status_handle = {
            let finished = finished.clone();
            std::thread::spawn(move || Self::run_indicator(finished, status_region))
        };

        for chunk in stream {
            match chunk? {
                CompletionChunk::Text(text) => {
                    let mut lock = self.inner.write();

                    let pos = insert_mark.read(&*lock).get_position()?;

                    lock.append_at_position(&pos, &text)?;
                }
            }
        }

        finished.store(true, Ordering::Relaxed);
        status_handle
            .join()
            .expect("Failed to join status thread")?;

        self.inner
            .write()
            .append(&format!("\n\n{USER_HEADER}\n\n"))?;

        Ok(())
    }
}
