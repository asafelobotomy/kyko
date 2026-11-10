//! Global hotkey through org.freedesktop.portal.GlobalShortcuts (raw zbus, blocking, own thread).
use std::collections::HashMap;

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};

const DEST: &str = "org.freedesktop.portal.Desktop";
const PATH: &str = "/org/freedesktop/portal/desktop";
const IFACE: &str = "org.freedesktop.portal.GlobalShortcuts";

type Dict = HashMap<String, OwnedValue>;

/// Call a portal method that answers through a Request object, and wait for its Response.
fn request<B>(conn: &Connection, portal: &Proxy<'_>, method: &str, token: &str, body: &B) -> Result<Dict, String>
where
    B: serde::Serialize + zbus::zvariant::DynamicType,
{
    let sender = conn.unique_name().ok_or("no unique name")?.trim_start_matches(':').replace('.', "_");
    let path = format!("/org/freedesktop/portal/desktop/request/{sender}/{token}");
    let req = Proxy::new(conn, DEST, path.as_str(), "org.freedesktop.portal.Request").map_err(|e| e.to_string())?;
    let mut responses = req.receive_signal("Response").map_err(|e| e.to_string())?;
    portal.call_method(method, body).map_err(|e| format!("{method}: {e}"))?;
    let msg = responses.next().ok_or("no Response")?;
    let (code, results): (u32, Dict) = msg.body().deserialize().map_err(|e| e.to_string())?;
    if code != 0 {
        return Err(format!("{method}: portal response {code} (1 = cancelled by the user, 2 = other)"));
    }
    Ok(results)
}

fn opts(pairs: &[(&'static str, &str)]) -> HashMap<&'static str, Value<'static>> {
    pairs.iter().map(|(k, v)| (*k, Value::from(v.to_string()))).collect()
}

/// Bind one shortcut `toggle`; returns the trigger description the portal reports.
pub fn start_portal_hotkey(preferred: &str, on_activated: impl Fn() + Send + 'static) -> Result<String, String> {
    let conn = Connection::session().map_err(|e| e.to_string())?;
    let portal = Proxy::new(&conn, DEST, PATH, IFACE).map_err(|_| "no GlobalShortcuts portal".to_string())?;
    let version: u32 = portal.get_property("version").map_err(|_| "no GlobalShortcuts portal".to_string())?;

    let created =
        request(&conn, &portal, "CreateSession", "kyko_c", &(opts(&[("handle_token", "kyko_c"), ("session_handle_token", "kyko_s")]),))?;
    let session: String =
        created.get("session_handle").and_then(|v| v.downcast_ref::<String>().ok()).ok_or("no session_handle")?;
    let session_path = ObjectPath::try_from(session.as_str()).map_err(|e| e.to_string())?;

    let shortcut = ("toggle", opts(&[("description", "Show or hide kyko"), ("preferred_trigger", preferred)]));
    let bound = request(
        &conn,
        &portal,
        "BindShortcuts",
        "kyko_b",
        &(&session_path, vec![shortcut], "", opts(&[("handle_token", "kyko_b")])),
    )?;
    let described = bound
        .get("shortcuts")
        .and_then(|v| v.try_clone().ok())
        .and_then(|v| <Vec<(String, Dict)>>::try_from(v).ok())
        .map(|list| {
            list.iter()
                .map(|(id, d)| {
                    let t = d.get("trigger_description").and_then(|v| v.downcast_ref::<String>().ok());
                    format!("{id}={}", t.unwrap_or_else(|| "<no trigger>".into()))
                })
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_else(|| "<no shortcuts in the response>".into());

    let mut activated = portal.receive_signal("Activated").map_err(|e| e.to_string())?;
    std::thread::spawn(move || {
        let _keep = conn;
        while let Some(msg) = activated.next() {
            if let Ok((_s, id, _t, _o)) = msg.body().deserialize::<(OwnedObjectPath, String, u64, Dict)>() {
                if id == "toggle" {
                    on_activated();
                }
            }
        }
    });
    Ok(format!("portal v{version}: {described}"))
}
