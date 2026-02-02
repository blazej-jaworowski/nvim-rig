use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use itertools::Itertools as _;
use regex::Regex;
use rig::message::Message;

use tracing::{debug, error, instrument};

use eel::{
    CompleteBufferHandle, Editor,
    buffer::{BufferHandle, ReadBuffer as _, WriteBuffer},
    cursor::CursorWriteBuffer,
    mark::Mark,
    region::BufferRegion,
};

use crate::{Result, completion::CompletionChunk, completion_cache::CompletionCache};

struct CompletionBufferInfo {
    model: String,
    prompt: String,
    history: Vec<Message>,
}

#[derive(Debug)]
pub struct CompletionBufferConfig {
    assisstant_header: String,
    user_header: String,

    available_models: HashMap<String, String>,
    default_model_name: String,
    default_model: String,

    model_header_format: String,
    model_header_regex: Regex,
}

impl Default for CompletionBufferConfig {
    fn default() -> Self {
        let available_models = [
            ("GeminiFlash", "google/gemini-3-flash-preview:online"),
            ("GeminiPro", "google/gemini-3-pro-preview:online"),
            ("ClaudeOpus", "anthropic/claude-opus-4.5:online"),
        ]
        .map(|(name, model)| (name.to_string(), model.to_string()));

        let (default_model_name, default_model) = available_models
            .first()
            .expect("No available models is invalid")
            .clone();

        let model_header_format = String::from("# Chosen model: %MODEL%");

        let regex = model_header_format.replace("%MODEL%", r"(\w+)");
        let regex = format!("^{regex}$");

        Self {
            assisstant_header: "# ** ----- Assistant ----- **".into(),
            user_header: "# ** ------- User -------- **".into(),
            available_models: HashMap::from(available_models),
            default_model_name,
            default_model,
            model_header_format,
            model_header_regex: Regex::new(&regex).expect("Invalid model header regex"),
        }
    }
}

impl CompletionBufferConfig {
    fn parse_model_line(&self, model_line: &str) -> Option<String> {
        let captures = self.model_header_regex.captures(model_line)?;
        let model_name = captures.get(1)?.as_str();

        Some(model_name.trim().to_string())
    }

    fn parse_content(&self, text: impl Iterator<Item = String>) -> CompletionBufferInfo {
        // TODO: This needs refactor

        let mut lines = text.peekable();

        let parsed_model = lines
            .peek()
            .map(String::as_str)
            .and_then(|l| self.parse_model_line(l));

        let model = if let Some(model) = parsed_model {
            debug!("Model provided: {model}");
            _ = lines.next();
            model
        } else {
            debug!(
                "Valid model not provided, using default: {}",
                self.default_model_name
            );
            self.default_model_name.to_string()
        };

        let model = self
            .available_models
            .get(&model)
            .unwrap_or_else(|| {
                error!(
                    "Invalid model: {model}, using default: {}",
                    self.default_model
                );
                &self.default_model
            })
            .clone();

        debug!("Using model: {model}");

        while let Some(line) = lines.peek() {
            if line.is_empty() {
                lines.next();
            } else {
                break;
            }
        }

        if let Some(line) = lines.peek() {
            if *line != self.user_header {
                // First non-empty line is not user header, treating the whole buffer as
                // unformatted user content
                return CompletionBufferInfo {
                    model,
                    prompt: lines.join("\n"),
                    history: Vec::new(),
                };
            }
        } else {
            // No lines
            return CompletionBufferInfo {
                model,
                prompt: "".into(),
                history: Vec::new(),
            };
        }

        let mut messages: Vec<Message> = Vec::new();
        let mut is_user_message = true;
        let mut partial_msg = String::new();

        for line in lines {
            // TODO: Ugly
            if line.is_empty() && partial_msg.is_empty() {
                continue;
            } else if line == self.assisstant_header {
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
            } else if line == self.user_header {
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
            } else {
                partial_msg.push_str(&line);
                partial_msg.push('\n');
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

    fn model_header(&self, model: &str) -> String {
        self.model_header_format.replace("%MODEL%", model)
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
    completion_cache: Arc<CompletionCache>,

    config: CompletionBufferConfig,
}

impl<E> CompletionBuffer<E>
where
    E: Editor,
    E::BufferHandle: CompleteBufferHandle,
{
    pub fn create_new(editor: Arc<E>, completion_cache: Arc<CompletionCache>) -> Result<Self> {
        let config = CompletionBufferConfig::default();
        let buffer = editor.new_buffer()?;
        {
            let mut lock = buffer.write();

            editor.set_current_buffer(&mut lock)?;

            lock.append(&config.model_header(&config.default_model_name))?;
            lock.append("\n\n")?;

            lock.append(&config.user_header)?;
            lock.append("\n\n")?;

            let pos = lock.max_pos()?;
            lock.set_cursor(&pos)?;
        }

        Ok(Self::create_from(buffer, completion_cache, config))
    }

    pub fn create_from(
        buffer: E::BufferHandle,
        completion_cache: Arc<CompletionCache>,
        config: CompletionBufferConfig,
    ) -> Self {
        Self {
            inner: buffer,
            completion_cache,
            config,
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
            self.config.parse_content(lines)
        };

        let agent = self.completion_cache.get_model(&info.model);

        let stream = agent.stream_chat(&info.prompt, info.history).into_iter();

        let mut lock = self.inner.write();

        lock.append("\n\n")?;
        lock.append(&self.config.assisstant_header)?;
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
        lock.append(&self.config.user_header)?;
        lock.append("\n\n")?;

        insert_result
    }
}
