//! GNOME plumbing shared by the Linux parts: calls on the session D-Bus and
//! GSettings. gio aborts the whole process on a schema that is not installed,
//! so every schema is looked up first and a missing one is just None.

use gio::glib::Variant;
use gio::prelude::*;

pub fn call(dest: &str, path: &str, interface: &str, method: &str, args: Option<&Variant>) -> Result<Variant, String> {
    let bus = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).map_err(|e| e.to_string())?;
    bus.call_sync(Some(dest), path, interface, method, args, None, gio::DBusCallFlags::NONE, 3000, gio::Cancellable::NONE)
        .map_err(|e| e.to_string())
}

/// `path` is for relocatable schemas (the custom keyboard shortcuts).
pub fn settings(schema: &str, path: Option<&str>) -> Option<gio::Settings> {
    let schema = gio::SettingsSchemaSource::default()?.lookup(schema, true)?;
    Some(gio::Settings::new_full(&schema, gio::SettingsBackend::NONE, path))
}

pub fn has_key(settings: &gio::Settings, key: &str) -> bool {
    settings.settings_schema().map(|s| s.has_key(key)).unwrap_or(false)
}

/// Writes go out asynchronously; this waits until they reached dconf.
pub fn sync() {
    gio::Settings::sync();
}
