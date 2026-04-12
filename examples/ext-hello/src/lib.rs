//! `ext-hello` — the reference arx extension.
//!
//! Registers a single command, `hello.greet`, that inserts
//! `"Hello from ext-hello!"` at the active cursor. Nothing fancy —
//! it exists so the extension host has something to load in tests
//! and so new users have a copy-pastable starting point.
//!
//! Build as a `cdylib` and drop into `~/.arx/extensions/` to use.

use arx_sdk::{
    ActivationContext, ActivationPolicy, Extension, ExtensionError, ExtensionMeta, SDK_VERSION,
    declare_extension,
};

#[derive(Debug, Default)]
pub struct HelloExtension;

impl Extension for HelloExtension {
    fn metadata(&self) -> ExtensionMeta {
        ExtensionMeta {
            name: "hello".into(),
            version: "0.1.0".into(),
            description: "Greets the user from a named command".into(),
            sdk_version: SDK_VERSION,
            activation: ActivationPolicy::Startup,
        }
    }

    fn activate(&self, ctx: &mut ActivationContext) -> Result<(), ExtensionError> {
        ctx.register_command(
            "hello.greet",
            "Insert a greeting at the cursor",
            |editor| {
                arx_sdk::core::stock::insert_at_cursor(editor, "Hello from ext-hello!");
                Ok(())
            },
        );
        Ok(())
    }
}

declare_extension!(HelloExtension);
