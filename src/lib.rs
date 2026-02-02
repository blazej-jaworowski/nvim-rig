mod error;

mod completion;
mod completion_buffer;
mod completion_cache;

mod api_key_utils;
pub use api_key_utils::{ApiKeyCache, pass_getter};

mod plugin;
pub use plugin::StaticRig;

pub mod nvim;

pub use completion_buffer::CompletionBuffer;

type Error = error::Error;
type Result<T> = std::result::Result<T, Error>;
