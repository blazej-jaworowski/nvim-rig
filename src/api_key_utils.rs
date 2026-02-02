use std::process::Command;

use tracing::debug;

use crate::plugin::ApiKeyGetter;

pub struct ApiKeyCache<T: ApiKeyGetter> {
    getter: T,
    api_key: Option<String>,
}

impl<T: ApiKeyGetter> ApiKeyCache<T> {
    pub fn new(getter: T) -> Self {
        Self {
            getter,
            api_key: None,
        }
    }
}

impl<T: ApiKeyGetter> ApiKeyGetter for ApiKeyCache<T> {
    fn api_key(&self) -> anyhow::Result<String> {
        if let Some(ref k) = self.api_key {
            return Ok(k.clone());
        }

        self.getter.api_key()
    }
}

pub fn pass_getter(store_location: String) -> impl ApiKeyGetter + 'static {
    let getter = move || {
        debug!("Getting API key using pass");

        let out = Command::new("pass")
            .args(["show", &store_location])
            .output()?
            .stdout;
        let api_key = str::from_utf8(out.as_slice())?.trim();

        Ok(api_key.into())
    };

    ApiKeyCache::new(getter)
}
