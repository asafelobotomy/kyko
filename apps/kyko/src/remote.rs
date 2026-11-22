//! The window's control socket for `kyko @` (see `kyko_core::remote`): a Unix socket only its
//! user can open, `$XDG_RUNTIME_DIR/kyko/rc-<pid>.sock`, named to each pane's programs in
//! `$KYKO_LISTEN_ON`. One JSON request per connection, one JSON reply.
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use kyko_core::remote::{Reply, Request};

/// The longest request read: `send-text` with a large paste.
const MAX_REQUEST: u64 = 4 << 20;

/// The socket asked for with `--listen-on`, set once at start.
static LISTEN_ON: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// `--listen-on unix:PATH` (or a bare path): where this window listens instead of
/// `rc-<pid>.sock`, so a launcher that opened it can reach it later with `kyko @ --to`.
pub fn set_listen_on(spec: &str) -> Result<(), String> {
    let path = spec.strip_prefix("unix:").unwrap_or(spec);
    if path.is_empty() || !path.starts_with('/') {
        return Err(format!("--listen-on {spec}: give unix:/absolute/path"));
    }
    let _ = LISTEN_ON.set(PathBuf::from(path));
    Ok(())
}

/// Where this window listens.
pub fn socket_path() -> PathBuf {
    if let Some(p) = LISTEN_ON.get() {
        return p.clone();
    }
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("kyko")
        .join(format!("rc-{}.sock", std::process::id()))
}

/// Listens at `path` and answers each request with `handle`, on a thread of its own. `None`
/// (with a message) when the socket cannot be made; kyko then runs without remote control.
pub fn serve(path: &Path, handle: impl Fn(Request) -> Reply + Send + 'static) -> Option<PathBuf> {
    let made = (|| -> std::io::Result<UnixListener> {
        if let Some(dir) = path.parent() {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir)?;
        }
        let _ = std::fs::remove_file(path);
        let listener = UnixListener::bind(path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        Ok(listener)
    })();
    let listener = match made {
        Ok(l) => l,
        Err(e) => {
            kyko_diag::problem(
                "kyko",
                kyko_diag::Level::WARN,
                &format!("[KYKO-002] remote control is off: {}: {e}", path.display()),
            );
            return None;
        }
    };
    std::thread::Builder::new()
        .name("kyko-rc".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let reply = match read_request(&stream) {
                    Ok(req) => handle(req),
                    Err(e) => Reply::err(e),
                };
                let mut stream = stream;
                let _ = serde_json::to_writer(&mut stream, &reply);
                let _ = stream.write_all(b"\n");
            }
        })
        .ok()?;
    Some(path.to_path_buf())
}

fn read_request(stream: &UnixStream) -> Result<Request, String> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut line = String::new();
    BufReader::new(stream.take(MAX_REQUEST))
        .read_line(&mut line)
        .map_err(|e| format!("reading the request: {e}"))?;
    serde_json::from_str(&line).map_err(|e| format!("not a request: {e}"))
}

/// Sends `request` to the window at `to` (`unix:PATH` or a path) and waits for its reply.
pub fn call(to: &str, request: &Request) -> Result<Reply, String> {
    let path = to.strip_prefix("unix:").unwrap_or(to);
    let mut stream = UnixStream::connect(path)
        .map_err(|e| format!("cannot reach the kyko window at {path}: {e}"))?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let mut text = serde_json::to_string(request).map_err(|e| e.to_string())?;
    text.push('\n');
    stream
        .write_all(text.as_bytes())
        .map_err(|e| format!("sending the request: {e}"))?;
    let mut reply = String::new();
    BufReader::new(stream)
        .read_line(&mut reply)
        .map_err(|e| format!("reading the reply: {e}"))?;
    serde_json::from_str(&reply).map_err(|e| format!("a reply kyko cannot read: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listen_on_takes_an_absolute_unix_path() {
        assert!(set_listen_on("unix:relative.sock").is_err());
        assert!(set_listen_on("").is_err());
        set_listen_on("unix:/run/user/1000/uq-dock.sock").unwrap();
        assert_eq!(socket_path(), PathBuf::from("/run/user/1000/uq-dock.sock"));
    }
    use kyko_core::remote::Match;

    #[test]
    fn a_request_gets_its_reply_and_only_the_user_can_connect() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/rc.sock");
        serve(&path, |req| match req {
            Request::Ls => Reply::ok(serde_json::json!({"tabs": []})),
            other => Reply::err(format!("{other:?}")),
        })
        .unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let dir_mode = std::fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(dir_mode & 0o777, 0o700);
        let to = format!("unix:{}", path.display());
        let r = call(&to, &Request::Ls).unwrap();
        assert!(r.ok);
        assert_eq!(r.data, serde_json::json!({"tabs": []}));
        let r = call(&to, &Request::ClosePane { target: Match::All }).unwrap();
        assert!(!r.ok);
        assert!(call("unix:/nonexistent/x.sock", &Request::Ls).is_err());
    }
}
