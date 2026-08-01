//! XDG **Background** portal glue: request (or drop) autostart-on-login when
//! running inside a Flatpak sandbox, where writing `~/.config/autostart`
//! directly is useless (that path is the app's private, host-invisible config).
//!
//! `org.freedesktop.portal.Background.RequestBackground` is an async
//! Request/Response call: the method returns a request-handle object path and
//! the actual outcome arrives later as a `Response` signal on that handle. To
//! avoid a race where the signal fires before we subscribe, we pass an explicit
//! `handle_token`, precompute the handle path, and start listening *before*
//! issuing the call — the pattern the portal documentation prescribes.
//!
//! This is platform glue (a live session bus + `xdg-desktop-portal`), exercised
//! manually rather than in unit tests; the pure decision helpers it relies on
//! (`is_flatpak`, `portal_autostart_command`, the marker file) live and are
//! tested in `fodder_core::autostart`.

use std::collections::HashMap;

use anyhow::{Context, Result};
use futures::StreamExt;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

/// Request autostart-on-login via the Background portal (or clear it when
/// `enabled` is false). Returns `Ok(true)` if the portal granted the request.
pub async fn request_autostart(enabled: bool, commandline: Vec<String>) -> Result<bool> {
    let conn = zbus::Connection::session()
        .await
        .context("connect to session bus")?;

    // Precompute the request-handle path from our unique bus name + a token, so
    // we can subscribe to the Response signal before the call can emit it.
    let unique = conn
        .unique_name()
        .context("session connection has no unique name")?
        .to_string();
    let sender = unique.trim_start_matches(':').replace('.', "_");
    let token = "fodder_autostart";
    let handle_path = format!("/org/freedesktop/portal/desktop/request/{sender}/{token}");

    let request = zbus::Proxy::new(
        &conn,
        "org.freedesktop.portal.Desktop",
        handle_path.as_str(),
        "org.freedesktop.portal.Request",
    )
    .await
    .context("build Request proxy")?;
    let mut responses = request
        .receive_signal("Response")
        .await
        .context("subscribe to portal Response")?;

    let background = zbus::Proxy::new(
        &conn,
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.Background",
    )
    .await
    .context("build Background proxy")?;

    let mut options: HashMap<&str, Value> = HashMap::new();
    options.insert("handle_token", Value::from(token));
    options.insert(
        "reason",
        Value::from("Poll feeds and show notifications in the background"),
    );
    options.insert("autostart", Value::from(enabled));
    // Ask to keep running in the background without an open window.
    options.insert("background", Value::from(enabled));
    options.insert("commandline", Value::from(commandline));
    options.insert("dbus-activatable", Value::from(false));

    // RequestBackground(parent_window: s, options: a{sv}) -> handle: o
    let returned: OwnedObjectPath = background
        .call("RequestBackground", &("", options))
        .await
        .context("RequestBackground call failed")?;
    if returned.as_str() != handle_path {
        tracing::debug!(
            "portal returned handle {} (expected {handle_path})",
            returned.as_str()
        );
    }

    // Await the Response signal: response == 0 means the request succeeded.
    let signal = responses
        .next()
        .await
        .context("portal closed without a Response")?;
    let (response, results): (u32, HashMap<String, OwnedValue>) = signal
        .body()
        .deserialize()
        .context("decode portal Response")?;
    let granted = results
        .get("autostart")
        .and_then(|v| bool::try_from(v).ok())
        .unwrap_or(enabled);
    Ok(response == 0 && (!enabled || granted))
}
