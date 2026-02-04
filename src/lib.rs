mod error;

mod completion;
mod completion_cache;

mod rig_buffer;
mod rig_buffer_format;

mod api_key_utils;
pub use api_key_utils::{ApiKeyCache, pass_getter};

mod plugin;
pub use plugin::StaticRig;

pub mod nvim;

pub use rig_buffer::RigBuffer;

type Error = error::Error;
type Result<T> = std::result::Result<T, Error>;
