use std::{
    collections::HashMap,
    sync::{
        Arc, LazyLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use itertools::Itertools as _;
use regex::Regex;
use rig::message::Message;

use tracing::{debug, instrument};

use eel::{
    CompleteBufferHandle, Editor,
    buffer::{BufferHandle, ReadBuffer as _, WriteBuffer},
    cursor::CursorWriteBuffer,
    mark::Mark,
    region::BufferRegion,
};

use crate::{Result, agent_cache::CompletionCache, completion::CompletionChunk};

pub const DEFAULT_MODEL_NAME: &str = "GeminiFlash";

static MODEL_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^# Chosen model: (\w+)$").expect("Invalid regex for header"));

static AVAILABLE_MODELS: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        ("GeminiFlash", "google/gemini-3-flash-preview"),
        ("GeminiPro", "google/gemini-3-pro-preview"),
        ("ClaudeOpus", "anthropic/claude-opus-4.5"),
    ])
});

static DEFAULT_MODEL: LazyLock<&'static str> = LazyLock::new(|| {
    AVAILABLE_MODELS
        .get(DEFAULT_MODEL_NAME)
        .expect("Invalid default model name")
});

const ASSISTANT_HEADER: &str = "# ** ----- Assistant ----- **";
const USER_HEADER: &str = "# ** ------- User -------- **";

struct CompletionBufferInfo {
    model: String,
    prompt: String,
    history: Vec<Message>,
}

impl CompletionBufferInfo {
    fn parse_content(text: impl Iterator<Item = String>) -> Self {
        // TODO: This needs refactor

        let mut lines = text.peekable();

        let model = if let Some(model_line) = lines.peek()
            && let Some(captures) = MODEL_REGEX.captures(model_line)
            && let Some(model_name) = captures.get(1)
            && let Some(model) = AVAILABLE_MODELS.get(model_name.as_str())
        {
            debug!("Model provided");
            _ = lines.next();
            model.trim().to_string()
        } else {
            debug!("Valid model not provided, using default");
            DEFAULT_MODEL.to_string()
        };

        debug!("Using model: {model}");

        while let Some(line) = lines.peek() {
            if line.is_empty() {
                lines.next();
            } else {
                break;
            }
        }

        // Valid content should start with USER_HEADER
        if !matches!(lines.peek().map(String::as_str), Some(USER_HEADER)) {
            return CompletionBufferInfo {
                model,
                prompt: lines.join("\n"),
                history: Vec::new(),
            };
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
            CompletionBufferInfo {
                model,
                prompt: partial_msg,
                history: messages,
            }
        } else {
            messages.push(Message::assistant(partial_msg));
            CompletionBufferInfo {
                model,
                prompt: String::new(),
                history: messages,
            }
        }
    }
}

#[derive(derivative::Derivative)]
#[derivative(Debug)]
pub struct CompletionBuffer<E>
where
    E: Editor,
{
    #[derivative(Debug = "ignore")]
    inner: E::BufferHandle,

    #[derivative(Debug = "ignore")]
    agent_cache: Arc<CompletionCache>,
}

impl<E> CompletionBuffer<E>
where
    E: Editor,
    E::BufferHandle: CompleteBufferHandle,
{
    pub fn create_new(editor: Arc<E>, agent_cache: Arc<CompletionCache>) -> Result<Self> {
        let buf = editor.new_buffer()?;
        {
            let mut buf = buf.write();

            buf.append(&format!("# Chosen model: {DEFAULT_MODEL_NAME}\n\n"))?;

            buf.append(&format!("{USER_HEADER}\n\n"))?;

            editor.set_current_buffer(&mut buf)?;

            let pos = buf.max_pos()?;
            buf.set_cursor(&pos)?;
        }

        Ok(Self::create_from(buf, agent_cache))
    }

    pub fn create_from(buf_handle: E::BufferHandle, agent_cache: Arc<CompletionCache>) -> Self {
        Self {
            inner: buf_handle,
            agent_cache,
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
        let info = {
            let lock = self.inner.read();
            let lines = lock.get_all_lines()?;
            CompletionBufferInfo::parse_content(lines)
        };

        let agent = self.agent_cache.get_model(&info.model);

        let stream = agent.stream_chat(&info.prompt, info.history).into_iter();

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

        let insert_result = Self::insert_response(stream, insert_mark);

        finished.store(true, Ordering::Relaxed);
        status_handle
            .join()
            .expect("Failed to join status thread")?;

        if insert_result.is_err() {
            self.inner.write().append("\n\nInference failed\n\n")?;
        }

        self.inner
            .write()
            .append(&format!("\n\n{USER_HEADER}\n\n"))?;

        insert_result
    }
}
