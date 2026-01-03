use crate::api_key::ApiKeyError;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("nvim-rig not initialized")]
    Uninitialized,

    #[error("Invalid model name")]
    InvalidModel,

    #[error("Failed to retreive API key: {0}")]
    ApiKey(#[from] ApiKeyError),

    #[error("Eel error: {0}")]
    Eel(#[from] eel::Error),

    #[error("Completion error: {0}")]
    Completion(#[from] crate::completion::Error),
}
