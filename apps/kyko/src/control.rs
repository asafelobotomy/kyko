//! `org.kyko.Terminal1` on the session bus: `Toggle`, `Show`, `Hide` and the `Visible` property,
//! served by the drop-down, and the small client the `kyko toggle|show|hide` commands use.
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use kyko_core::tabsearch::{self, TabInfo};
use zbus::object_server::SignalEmitter;
use zbus::{
    Connection, connection,
    fdo::{RequestNameFlags, RequestNameReply},
};

pub const NAME: &str = "org.kyko.Terminal1";
pub const PATH: &str = "/org/kyko/Terminal1";
pub const LAUNCHER_PATH: &str = "/org/uq/Launcher1";

/// What the bus asks of the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Toggle,
    Show,
    Hide,
}

impl Command {
    /// The number the C++ side carries it as.
    pub fn code(self) -> i32 {
        match self {
            Command::Toggle => 0,
            Command::Show => 1,
            Command::Hide => 2,
        }
    }

    pub fn method(self) -> &'static str {
        match self {
            Command::Toggle => "Toggle",
            Command::Show => "Show",
            Command::Hide => "Hide",
        }
    }
}

/// A request that carries data: `Run` and `NewTab`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// Run `command` in a new tab; `cwd` is empty for the default folder.
    Run { command: String, cwd: String },
    /// A new tab with the shell.
    NewTab { cwd: String },
}

/// What the window answers the bus with.
pub trait Host: Send + Sync + 'static {
    fn command(&self, command: Command);
    fn request(&self, request: Request) -> Result<(), String>;
    /// The open tabs.
    fn tabs(&self) -> Vec<TabInfo>;
    /// Focuses and shows a tab; false when there is no such tab.
    fn focus_tab(&self, id: u32) -> bool;
}

struct Terminal {
    visible: Arc<AtomicBool>,
    host: Arc<dyn Host>,
}

#[zbus::interface(name = "org.kyko.Terminal1")]
impl Terminal {
    fn toggle(&self) {
        self.host.command(Command::Toggle);
    }

    fn show(&self) {
        self.host.command(Command::Show);
    }

    fn hide(&self) {
        self.host.command(Command::Hide);
    }

    fn run(&self, command: String, cwd: String) -> zbus::fdo::Result<()> {
        self.host
            .request(Request::Run { command, cwd })
            .map_err(zbus::fdo::Error::InvalidArgs)
    }

    fn new_tab(&self, cwd: String) -> zbus::fdo::Result<()> {
        self.host
            .request(Request::NewTab { cwd })
            .map_err(zbus::fdo::Error::InvalidArgs)
    }

    /// `a(uss)`: id, name, the foreground program.
    fn list_tabs(&self) -> Vec<(u32, String, String)> {
        self.host
            .tabs()
            .into_iter()
            .map(|t| (t.id, t.name, t.program))
            .collect()
    }

    fn focus_tab(&self, id: u32) -> zbus::fdo::Result<()> {
        if self.host.focus_tab(id) {
            Ok(())
        } else {
            Err(zbus::fdo::Error::InvalidArgs(format!("no tab {id}")))
        }
    }

    #[zbus(property)]
    fn visible(&self) -> bool {
        self.visible.load(Ordering::SeqCst)
    }
}

/// kyko as a uQ launcher provider: Spotlight lists the open tabs (this never runs a command).
struct Provider {
    host: Arc<dyn Host>,
}

#[zbus::interface(name = "org.uq.Launcher1.Provider")]
impl Provider {
    /// `a(sssssd)`: id, title, subtitle, icon, actions (JSON list), score.
    fn search(&self, query: String) -> Vec<(String, String, String, String, String, f64)> {
        tabsearch::search(&self.host.tabs(), &query)
            .into_iter()
            .map(|h| {
                (
                    h.id.to_string(),
                    h.title,
                    h.subtitle,
                    "utilities-terminal".to_string(),
                    "[]".to_string(),
                    h.score,
                )
            })
            .collect()
    }

    /// Only an id that `search` returned for a tab that still exists; it only focuses the tab.
    fn activate(&self, id: String, _action: String) -> zbus::fdo::Result<()> {
        let id: u32 = id
            .parse()
            .map_err(|_| zbus::fdo::Error::InvalidArgs("not a tab id".into()))?;
        if self.host.focus_tab(id) {
            Ok(())
        } else {
            Err(zbus::fdo::Error::InvalidArgs(format!("no tab {id}")))
        }
    }
}

/// The served object; keeps the connection alive.
pub struct Service {
    connection: Connection,
    visible: Arc<AtomicBool>,
}

/// Takes the name and serves the object, or says why not (another kyko already owns it).
pub fn serve(host: Arc<dyn Host>) -> zbus::Result<Service> {
    let visible = Arc::new(AtomicBool::new(false));
    let iface = Terminal {
        visible: visible.clone(),
        host: host.clone(),
    };
    let provider = Provider { host };
    let connection = zbus::block_on(async {
        connection::Builder::session()
            .serve_at(PATH, iface)
            .serve_at(LAUNCHER_PATH, provider)
            .build()
            .await
    })?;
    // Without queueing: a second drop-down must fail instead of waiting for the first to leave.
    let reply =
        zbus::block_on(connection.request_name_with_flags(NAME, RequestNameFlags::DO_NOT_QUEUE))?;
    if !matches!(
        reply,
        RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner
    ) {
        return Err(zbus::Error::NameTaken);
    }
    Ok(Service {
        connection,
        visible,
    })
}

impl Service {
    /// Records the window's visibility and tells watchers of the property.
    pub fn set_visible(&self, visible: bool) {
        if self.visible.swap(visible, Ordering::SeqCst) == visible {
            return;
        }
        let _ = zbus::block_on(async {
            let iface = self
                .connection
                .object_server()
                .interface::<_, Terminal>(PATH)
                .await?;
            let emitter: SignalEmitter<'_> = iface.signal_emitter().clone();
            iface.get().await.visible_changed(&emitter).await
        });
    }
}

/// Asks a running drop-down to do `command`. `Err` carries a message for the user.
pub fn call(command: Command) -> Result<(), String> {
    zbus::block_on(async {
        let conn = connect().await?;
        conn.call_method(Some(NAME), PATH, Some(NAME), command.method(), &())
            .await
            .map(|_| ())
            .map_err(explain)
    })
}

/// Asks a running drop-down for a new tab (`Run` or `NewTab`).
pub fn request(request: &Request) -> Result<(), String> {
    zbus::block_on(async {
        let conn = connect().await?;
        match request {
            Request::Run { command, cwd } => {
                conn.call_method(
                    Some(NAME),
                    PATH,
                    Some(NAME),
                    "Run",
                    &(command.as_str(), cwd.as_str()),
                )
                .await
            }
            Request::NewTab { cwd } => {
                conn.call_method(Some(NAME), PATH, Some(NAME), "NewTab", &(cwd.as_str(),))
                    .await
            }
        }
        .map(|_| ())
        .map_err(explain)
    })
}

async fn connect() -> Result<Connection, String> {
    Connection::session()
        .await
        .map_err(|e| format!("no session bus: {e}"))
}

/// What a failed call says to the user: the drop-down's own refusal, or that it is not running.
fn explain(e: zbus::Error) -> String {
    const NOT_RUNNING: &str = "kyko's drop-down is not running (start it with `kyko dropdown`)";
    match e {
        zbus::Error::MethodError(name, detail, _) => {
            if name.as_str().ends_with("InvalidArgs") {
                detail.unwrap_or_else(|| "refused".to_string())
            } else {
                NOT_RUNNING.to_string()
            }
        }
        zbus::Error::FDO(inner) => match *inner {
            zbus::fdo::Error::InvalidArgs(m) => m,
            _ => NOT_RUNNING.to_string(),
        },
        other => other.to_string(),
    }
}
