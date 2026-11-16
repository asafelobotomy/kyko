//! `org.kyko.Terminal1` on the session bus: `Toggle`, `Show`, `Hide` and the `Visible` property,
//! served by the drop-down, and the small client the `kyko toggle|show|hide` commands use.
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use zbus::object_server::SignalEmitter;
use zbus::{
    Connection, connection,
    fdo::{RequestNameFlags, RequestNameReply},
};

pub const NAME: &str = "org.kyko.Terminal1";
pub const PATH: &str = "/org/kyko/Terminal1";

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

struct Terminal {
    visible: Arc<AtomicBool>,
    send: Box<dyn Fn(Command) + Send + Sync>,
}

#[zbus::interface(name = "org.kyko.Terminal1")]
impl Terminal {
    fn toggle(&self) {
        (self.send)(Command::Toggle);
    }

    fn show(&self) {
        (self.send)(Command::Show);
    }

    fn hide(&self) {
        (self.send)(Command::Hide);
    }

    #[zbus(property)]
    fn visible(&self) -> bool {
        self.visible.load(Ordering::SeqCst)
    }
}

/// The served object; keeps the connection alive.
pub struct Service {
    connection: Connection,
    visible: Arc<AtomicBool>,
}

/// Takes the name and serves the object, or says why not (another kyko already owns it).
pub fn serve(send: impl Fn(Command) + Send + Sync + 'static) -> zbus::Result<Service> {
    let visible = Arc::new(AtomicBool::new(false));
    let iface = Terminal {
        visible: visible.clone(),
        send: Box::new(send),
    };
    let connection = zbus::block_on(async {
        connection::Builder::session()
            .serve_at(PATH, iface)
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
        let conn = Connection::session()
            .await
            .map_err(|e| format!("no session bus: {e}"))?;
        conn.call_method(Some(NAME), PATH, Some(NAME), command.method(), &())
            .await
            .map(|_| ())
            .map_err(|e| match e {
                zbus::Error::MethodError(..) | zbus::Error::FDO(_) => {
                    "kyko's drop-down is not running (start it with `kyko dropdown`)".to_string()
                }
                other => other.to_string(),
            })
    })
}
