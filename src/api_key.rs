use std::process::Command;

use tracing::debug;

#[derive(thiserror::Error, Debug)]
pub enum ApiKeyError {
    #[error("pass execution failed: {0}")]
    PassFailed(#[from] std::io::Error),

    #[error("Pass output decode error")]
    Decode(#[from] std::str::Utf8Error),
}

pub fn get_api_key(store_location: &str) -> Result<String, ApiKeyError> {
    debug!("Getting API key");

    let out = Command::new("pass")
        .args(["show", store_location])
        .output()?
        .stdout;
    let api_key = str::from_utf8(out.as_slice())?.trim();

    Ok(api_key.into())
}
