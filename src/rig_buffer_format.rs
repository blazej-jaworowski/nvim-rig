use std::collections::HashMap;

use genawaiter::{rc::r#gen, yield_};
use regex::Regex;
use rig::message::Message;
use tracing::debug;

use crate::completion_cache::CompletionConfig;

#[derive(Debug)]
struct AvailableModels {
    models: HashMap<String, String>,
    pub default: String,
}

impl AvailableModels {
    fn new(models: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>) -> Self {
        let mut models = models
            .into_iter()
            .map(|(name, model)| (name.into(), model.into()))
            .peekable();

        // The default model will be the first provided
        let default = models.peek().expect("Models can't be empty").1.clone();

        Self {
            models: HashMap::from_iter(models),
            default,
        }
    }

    fn get_model<'a>(&'a self, model_name: Option<&'a str>) -> &'a str {
        let model_name = match model_name {
            Some(n) => n,
            None => {
                debug!("No model name provided, using default: {}", self.default);
                return &self.default;
            }
        };

        debug!("Model name provided: {model_name}");

        self.models
            .get(model_name)
            .map(|s| s.as_str())
            // If there is no such name, we assume a model string was provided
            .unwrap_or_else(|| {
                debug!("Provided model name not in available models, using as is");
                model_name
            })
    }
}

impl Default for AvailableModels {
    fn default() -> Self {
        Self::new([
            ("GeminiFlash", "google/gemini-3-flash-preview:online"),
            ("GeminiPro", "google/gemini-3-pro-preview:online"),
            ("ClaudeOpus", "anthropic/claude-opus-4.5:online"),
        ])
    }
}

#[derive(Debug)]
struct FormattedProperty {
    format: String,
    regex: Regex,
}

impl FormattedProperty {
    const PLACEHOLDER: &'static str = "%PLACEHOLDER%";

    fn new(format: impl Into<String>, on_start: bool) -> Self {
        let format = format.into();

        assert!(
            format.contains(Self::PLACEHOLDER),
            "Format string has to have a placeholder"
        );
        assert_eq!(
            format.find(Self::PLACEHOLDER),
            format.rfind(Self::PLACEHOLDER),
            "Format string has to only have a single placeholder"
        );

        // The regex will match the shortest amount of characters behind the placeholder.
        // Make sure that stuff after the placeholder will never match what's inside.
        let regex_str = regex::escape(&format);
        let regex_str = regex_str.replace(Self::PLACEHOLDER, r"(.+?)");
        let regex_str = if on_start {
            format!(r"(?s)^\s*{regex_str}")
        } else {
            format!(r"(?s)\s*{regex_str}")
        };

        let regex = Regex::new(&regex_str).expect("Invalid regex");

        Self { format, regex }
    }

    fn match_text<'a>(&self, text: &'a str) -> Option<(&'a str, usize, usize)> {
        let captures = self.regex.captures(text)?;

        let whole = captures.get(0).expect("Capture 0 should always be present");
        let value = captures
            .get(1)
            .expect("Regex should have a single capture")
            .as_str();

        Some((value, whole.start(), whole.end()))
    }

    fn format(&self, value: &str) -> String {
        self.format.replace(Self::PLACEHOLDER, value)
    }
}

#[derive(Debug)]
pub struct RigBufferInfo {
    pub completion_config: CompletionConfig,
    pub messages: Vec<Message>,
}

#[derive(Debug)]
pub struct RigBufferFormat {
    available_models: AvailableModels,
    default_preamble: String,

    model_format: FormattedProperty,
    preamble_format: FormattedProperty,
    message_format: FormattedProperty,
}

impl Default for RigBufferFormat {
    fn default() -> Self {
        Self {
            available_models: AvailableModels::default(),
            default_preamble: r#"
### Role and Persona
You are a precise and structured AI assistant. Your goal is to provide clear, accurate responses.

### Output Format Guidelines
**Markdown Structure**:
- Use Markdown for all output.
- **Never** use Heading Level 1 (#).
- Start all section headers at Heading Level 2 (##) or deeper (###, ####).
"#
            .to_string(),

            model_format: FormattedProperty::new("Chosen model: `%PLACEHOLDER%`", true),
            preamble_format: FormattedProperty::new(
                r#"Preamble:
```
%PLACEHOLDER%
```"#,
                true,
            ),
            message_format: FormattedProperty::new("# **--- %PLACEHOLDER% ---**", false),
        }
    }
}

impl RigBufferFormat {
    fn parse_messages(&self, mut text: &str) -> impl IntoIterator<Item = Message> {
        r#gen! {{
            let mut is_user_msg: bool = true;

            while let Some((name, start, end)) = self.message_format.match_text(text) {
                let msg = text[..start].to_string();

                if !msg.is_empty() {
                    if is_user_msg {
                        yield_!(Message::user(msg));
                    } else {
                        yield_!(Message::assistant(msg));
                    }
                }

                is_user_msg = name != "Assisstant";
                text = &text[end..];
            }

            if is_user_msg {
                yield_!(Message::user(text));
            } else {
                yield_!(Message::assistant(text));
            }
        }}
    }

    pub(crate) fn parse_content(&self, text: &str) -> RigBufferInfo {
        let (model, text) = match self.model_format.match_text(text) {
            Some((m, _, end)) => (Some(m), &text[end..]),
            None => (None, text),
        };
        let model = self.available_models.get_model(model);

        let (preamble, text) = match self.preamble_format.match_text(text) {
            Some((p, _, end)) => (Some(p), &text[end..]),
            None => (None, text),
        };
        let preamble = preamble.unwrap_or_else(|| {
            debug!("No preamble provided, using default");
            &self.default_preamble
        });

        debug!("Model: '{model}'");
        debug!("Preamble: '{preamble}'");

        let messages = Vec::from_iter(self.parse_messages(text));

        debug!("Messages: {messages:?}");

        RigBufferInfo {
            completion_config: CompletionConfig::new(model, preamble),
            messages,
        }
    }

    pub(crate) fn default_buffer_content(&self) -> String {
        format!(
            "{}\n\n{}\n\n{}\n\n",
            self.model_format.format(&self.available_models.default),
            self.preamble_format.format(&self.default_preamble),
            self.user_header(),
        )
    }

    pub(crate) fn assisstant_header(&self) -> String {
        self.message_format.format("Assisstant")
    }

    pub(crate) fn user_header(&self) -> String {
        self.message_format.format("User")
    }
}
