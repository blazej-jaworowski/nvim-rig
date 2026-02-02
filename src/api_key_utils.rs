use std::process::Command;

use parking_lot::Mutex;
use tracing::debug;

use crate::plugin::ApiKeyGetter;

pub struct ApiKeyCache<T: ApiKeyGetter> {
    getter: T,
    api_key: Mutex<Option<String>>,
}

impl<T: ApiKeyGetter> ApiKeyCache<T> {
    pub fn new(getter: T) -> Self {
        Self {
            getter,
            api_key: Mutex::new(None),
        }
    }
}

impl<T: ApiKeyGetter> ApiKeyGetter for ApiKeyCache<T> {
    fn api_key(&self) -> anyhow::Result<String> {
        if let Some(ref k) = *self.api_key.lock() {
            return Ok(k.clone());
        }

        let key = self.getter.api_key()?;

        *self.api_key.lock() = Some(key.clone());

        Ok(key)
    }
}

pub fn pass_getter(store_location: String) -> impl ApiKeyGetter + 'static {
    let getter = move || -> anyhow::Result<String> {
        debug!("Getting API key using pass");

        let out = Command::new("pass")
            .args(["show", &store_location])
            .output()?;

        if !out.status.success() {
            let error_msg = format!(
                "pass command failed with status {}: {}",
                out.status,
                str::from_utf8(out.stderr.as_slice())?,
            );
            return Err(anyhow::Error::msg(error_msg));
        }

        let api_key = str::from_utf8(out.stdout.as_slice())?.trim();

        Ok(api_key.into())
    };

    ApiKeyCache::new(getter)
}
