//! Serving `org.kyko.Terminal1`: the bus host that routes other programs' commands and requests to the window.

use super::*;

pub(crate) struct BusHost;

impl crate::control::Host for BusHost {
    fn command(&self, command: crate::control::Command) {
        ffi::controller_send(command.code());
    }

    fn request(&self, request: crate::control::Request) -> Result<(), String> {
        handle_request(request)
    }

    fn tabs(&self) -> Vec<kyko_core::tabsearch::TabInfo> {
        let layout = WORKSPACE
            .get()
            .map(|w| w.shared.lock().unwrap().layout.clone())
            .unwrap_or_default();
        tabs_from_layout(&layout)
    }

    fn focus_tab(&self, id: u32) -> bool {
        if !self.tabs().iter().any(|t| t.id == id) {
            return false;
        }
        let Some(w) = WORKSPACE.get() else {
            return false;
        };
        w.act(Act::FocusTab(id));
        ffi::controller_send(crate::control::Command::Show.code());
        true
    }
}

/// The tabs in the layout JSON the window publishes.
pub(crate) fn tabs_from_layout(layout: &str) -> Vec<kyko_core::tabsearch::TabInfo> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(layout) else {
        return Vec::new();
    };
    let text = |t: &serde_json::Value, k: &str| t[k].as_str().unwrap_or_default().to_string();
    v["tabs"]
        .as_array()
        .map(|tabs| {
            tabs.iter()
                .filter_map(|t| {
                    Some(kyko_core::tabsearch::TabInfo {
                        id: u32::try_from(t["id"].as_u64()?).ok()?,
                        name: text(t, "name"),
                        program: text(t, "program"),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `Run` and `NewTab` from the bus: a new tab in the window, then the window shown.
pub(crate) fn handle_request(req: crate::control::Request) -> Result<(), String> {
    use crate::control::Request;
    let Some(w) = WORKSPACE.get() else {
        return Err("kyko is still starting".into());
    };
    let act = match req {
        Request::Run { command, cwd } => {
            if !look().allow_run {
                return Err("kyko has `advanced.allow_run` turned off".into());
            }
            kyko_core::runq::validate(&command).map_err(|e| e.to_string())?;
            Act::Run {
                command: Some(command),
                cwd: kyko_core::runq::cwd(&cwd),
            }
        }
        Request::NewTab { cwd } => Act::Run {
            command: None,
            cwd: kyko_core::runq::cwd(&cwd),
        },
    };
    w.act(act);
    ffi::controller_send(crate::control::Command::Show.code());
    Ok(())
}
