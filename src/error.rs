#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("nvim-rig not initialized")]
    Uninitialized,

    #[error("Failed to retreive API key: {0}")]
    ApiKey(anyhow::Error),

    #[error("Eel error: {0}")]
    Eel(#[from] eel::Error),

    #[error("IO error")]
    IO(#[from] std::io::Error),

    #[error("Http client error")]
    HttpClient(#[from] rig::http_client::Error),

    #[error("Completion error: {0}")]
    Completion(#[from] crate::completion::Error),
}
