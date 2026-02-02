use std::sync::{Arc, OnceLock};

use eel_nvim::editor::NvimEditor;
use tracing::debug;

use crate::{
    Result,
    plugin::{ApiKeyGetter, Plugin},
};

pub use crate::StaticRig;

static PLUGIN: OnceLock<Plugin<NvimEditor>> = OnceLock::new();

pub struct NvimRig;

impl StaticRig<NvimEditor> for NvimRig {
    fn get_instance() -> Result<&'static Plugin<NvimEditor>> {
        PLUGIN.get().ok_or(crate::Error::Uninitialized)
    }

    fn setup(editor: Arc<NvimEditor>, api_key_getter: impl ApiKeyGetter + 'static) -> Result<()> {
        debug!("Initializing nvim-rig");

        _ = PLUGIN
            .set(Plugin::new(editor, api_key_getter)?)
            .inspect_err(|_| tracing::warn!("Rig setup called more than once"));

        Ok(())
    }
}
