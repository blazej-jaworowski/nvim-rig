use crate::api_key::ApiKeyError;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("nvim-rig not initialized")]
    Uninitialized,

    #[error("Invalid model name")]
    InvalidModel,

    #[error("Failed to retreive API key: {0}")]
    ApiKey(#[from] ApiKeyError),

    #[error("Buffer error: {0}")]
    Buffer(#[from] nvim_api_helper::buffer::Error),

    #[error("Nvim dispatch error: {0}")]
    NvimDispatch(#[from] nvim_api_helper::nvim::async_dispatch::Error),

    #[error("Completion error: {0}")]
    Completion(#[from] crate::completion::Error),
}
