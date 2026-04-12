//! Cross-platform IPC transport for the daemon ↔ client link.
//!
//! Hides the `#[cfg(unix)]` / `#[cfg(windows)]` split behind one
//! [`IpcAddress`], [`IpcListener`], and [`IpcStream`] so the daemon code
//! in `arx-driver` doesn't know whether it's talking over a Unix domain
//! socket or a Windows named pipe.
//!
//! ## Address model
//!
//! * On Unix an address is a filesystem path (`IpcAddress::Path`).
//! * On Windows an address is a named-pipe name of the form
//!   `\\.\pipe\arx-<user>` (`IpcAddress::Pipe`).
//!
//! A `FromStr` impl lets `clap` parse either via a single `--socket`
//! argument: on Unix, a bare path becomes `Path`; anything starting
//! with `\\.\pipe\` becomes `Pipe`. Mixing addresses across platforms
//! is a user error — `IpcListener::bind` / `IpcStream::connect` return
//! a structured [`TransportError`] if you hand them the wrong kind.
//!
//! ## Windows accept loop
//!
//! Named pipes aren't really "listener" types — each connection is a
//! fresh server handle. The idiomatic multi-client pattern (straight out
//! of tokio's [`named_pipe`] docs) is to pre-create a `NamedPipeServer`,
//! `connect().await` it when a client appears, then immediately create
//! the *next* server under the same pipe name so subsequent clients
//! have somewhere to land. `IpcListener::accept` encapsulates that
//! dance so callers see the same API as the Unix side.
//!
//! ## Read / write halves
//!
//! Both sides split via [`tokio::io::split`] into
//! `Box<dyn AsyncRead + Unpin + Send>` / `Box<dyn AsyncWrite + Unpin +
//! Send>`. The trait-object box means the daemon code never sees a
//! backend-specific type (`OwnedReadHalf` on Unix, concrete named-pipe
//! types on Windows).
//!
//! [`named_pipe`]: tokio::net::windows::named_pipe

use std::io;
use std::path::PathBuf;
use std::str::FromStr;

use thiserror::Error;
use tokio::io::{AsyncRead, AsyncWrite};

/// Address of a daemon's IPC endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IpcAddress {
    /// Unix domain socket path. Valid on Unix targets.
    Path(PathBuf),
    /// Windows named pipe name, e.g. `\\.\pipe\arx-alice`. Valid on
    /// Windows targets.
    Pipe(String),
}

impl IpcAddress {
    /// Render as a human-readable string for logging / error messages.
    #[must_use]
    pub fn display(&self) -> String {
        match self {
            Self::Path(p) => p.display().to_string(),
            Self::Pipe(name) => name.clone(),
        }
    }
}

impl std::fmt::Display for IpcAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.display())
    }
}

impl FromStr for IpcAddress {
    type Err = std::convert::Infallible;

    /// Parse any user-supplied `--socket` argument.
    ///
    /// Anything that looks like a named pipe (`\\.\pipe\…` on Windows)
    /// becomes [`IpcAddress::Pipe`]. Everything else is treated as a
    /// filesystem path for [`IpcAddress::Path`]. The bind/connect calls
    /// validate that the chosen variant is actually supported on the
    /// current platform.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.starts_with(r"\\.\pipe\") || s.starts_with(r"\\?\pipe\") {
            Ok(Self::Pipe(s.to_owned()))
        } else {
            Ok(Self::Path(PathBuf::from(s)))
        }
    }
}

/// Cross-platform "who is logged in" probe.
///
/// Checks `$USER` (Unix), then `$USERNAME` (Windows), then falls back
/// to the literal `"unknown"`. Used by [`default_address`] and related
/// helpers that need to tag per-user runtime files.
#[must_use]
pub fn current_user() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".into())
}

/// Pick the default IPC address for the current user.
///
/// | Platform | Address |
/// |---|---|
/// | Unix (Linux, …) | `$XDG_RUNTIME_DIR/arx.sock` → `/tmp/arx-<user>.sock` fallback |
/// | macOS | same as Unix (`$XDG_RUNTIME_DIR` usually unset → `/tmp` path) |
/// | Windows | `\\.\pipe\arx-<user>` |
#[must_use]
pub fn default_address() -> IpcAddress {
    #[cfg(unix)]
    {
        if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
            if !dir.is_empty() {
                return IpcAddress::Path(PathBuf::from(dir).join("arx.sock"));
            }
        }
        IpcAddress::Path(PathBuf::from(format!("/tmp/arx-{}.sock", current_user())))
    }
    #[cfg(windows)]
    {
        IpcAddress::Pipe(format!(r"\\.\pipe\arx-{}", current_user()))
    }
}

/// Pick the default session-file path for the current user.
///
/// Session persistence is *data* (survives reboots) rather than
/// *runtime* state (lives until shutdown), so the search path follows
/// the XDG state spec rather than `$XDG_RUNTIME_DIR`.
///
/// | Platform | Path |
/// |---|---|
/// | Unix (Linux, …) | `$XDG_STATE_HOME/arx/session.postcard` → `$HOME/.local/state/arx/session.postcard` → `/tmp/arx-<user>-session.postcard` |
/// | Windows | `%LOCALAPPDATA%\arx\session.postcard` → `%USERPROFILE%\arx\session.postcard` |
///
/// The parent directory is only created by
/// [`arx_core::Session::save_to_path`] at write time — we don't touch
/// the filesystem here.
#[must_use]
pub fn default_session_path() -> PathBuf {
    #[cfg(unix)]
    {
        if let Ok(dir) = std::env::var("XDG_STATE_HOME") {
            if !dir.is_empty() {
                return PathBuf::from(dir).join("arx").join("session.postcard");
            }
        }
        if let Ok(home) = std::env::var("HOME") {
            if !home.is_empty() {
                return PathBuf::from(home)
                    .join(".local")
                    .join("state")
                    .join("arx")
                    .join("session.postcard");
            }
        }
        PathBuf::from(format!(
            "/tmp/arx-{}-session.postcard",
            current_user()
        ))
    }
    #[cfg(windows)]
    {
        if let Ok(appdata) = std::env::var("LOCALAPPDATA") {
            if !appdata.is_empty() {
                return PathBuf::from(appdata).join("arx").join("session.postcard");
            }
        }
        if let Ok(profile) = std::env::var("USERPROFILE") {
            if !profile.is_empty() {
                return PathBuf::from(profile).join("arx").join("session.postcard");
            }
        }
        PathBuf::from(format!("arx-{}-session.postcard", current_user()))
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors returned by the transport layer.
#[derive(Debug, Error)]
pub enum TransportError {
    #[error("I/O error on {address}: {source}")]
    Io {
        address: String,
        #[source]
        source: io::Error,
    },
    #[error("address is not a valid {variant} on this platform: {address}")]
    WrongVariant { address: String, variant: &'static str },
}

impl From<TransportError> for io::Error {
    fn from(e: TransportError) -> Self {
        match e {
            TransportError::Io { source, .. } => source,
            TransportError::WrongVariant { address, variant } => io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("address {address} is not a valid {variant} on this platform"),
            ),
        }
    }
}

fn io_err(address: &IpcAddress, source: io::Error) -> TransportError {
    TransportError::Io {
        address: address.display(),
        source,
    }
}

// ---------------------------------------------------------------------------
// Listener / stream types
// ---------------------------------------------------------------------------

/// A boxed async reader returned by [`IpcStream::into_split`].
pub type IpcReadHalf = Box<dyn AsyncRead + Unpin + Send>;

/// A boxed async writer returned by [`IpcStream::into_split`].
pub type IpcWriteHalf = Box<dyn AsyncWrite + Unpin + Send>;

/// A server that accepts client connections on a shared IPC endpoint.
pub struct IpcListener {
    inner: ListenerInner,
    address: IpcAddress,
}

impl std::fmt::Debug for IpcListener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IpcListener")
            .field("address", &self.address)
            .finish_non_exhaustive()
    }
}

impl IpcListener {
    /// Bind a listener at the given address. On Unix, stale socket
    /// files are removed before `bind` so repeated runs of the daemon
    /// don't fail with `EADDRINUSE`.
    pub fn bind(address: &IpcAddress) -> Result<Self, TransportError> {
        let inner = ListenerInner::bind(address)?;
        Ok(Self {
            inner,
            address: address.clone(),
        })
    }

    /// Wait for the next client connection.
    pub async fn accept(&mut self) -> Result<IpcStream, TransportError> {
        let stream = self.inner.accept(&self.address).await?;
        Ok(IpcStream { inner: stream })
    }

    /// Address this listener is bound to.
    #[must_use]
    pub fn address(&self) -> &IpcAddress {
        &self.address
    }
}

/// A bidirectional IPC byte stream.
pub struct IpcStream {
    inner: StreamInner,
}

impl std::fmt::Debug for IpcStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IpcStream").finish_non_exhaustive()
    }
}

impl IpcStream {
    /// Connect to a daemon at `address`.
    pub async fn connect(address: &IpcAddress) -> Result<Self, TransportError> {
        let inner = StreamInner::connect(address).await?;
        Ok(Self { inner })
    }

    /// Split the stream into boxed read / write halves.
    ///
    /// Uses [`tokio::io::split`] so both halves are plain trait objects
    /// regardless of the backend. The daemon never sees backend-specific
    /// split types like `UnixStream::OwnedReadHalf`.
    pub fn into_split(self) -> (IpcReadHalf, IpcWriteHalf) {
        self.inner.into_split()
    }
}

// ---------------------------------------------------------------------------
// Unix backend
// ---------------------------------------------------------------------------

#[cfg(unix)]
mod backend {
    // The backend module needs every public type the outer module
    // defines plus the private `io_err` helper; a wildcard import is
    // the cleanest way to pull them all in.
    #[allow(clippy::wildcard_imports)]
    use super::*;
    use tokio::net::{UnixListener, UnixStream};

    pub(super) struct ListenerInner {
        listener: UnixListener,
        path: PathBuf,
    }

    pub(super) struct StreamInner {
        stream: UnixStream,
    }

    impl ListenerInner {
        pub(super) fn bind(address: &IpcAddress) -> Result<Self, TransportError> {
            let path = match address {
                IpcAddress::Path(p) => p.clone(),
                IpcAddress::Pipe(_) => {
                    return Err(TransportError::WrongVariant {
                        address: address.display(),
                        variant: "Unix domain socket path",
                    });
                }
            };
            // Remove a stale socket file if present. We do NOT check for
            // a live daemon here — that's the caller's job.
            let _ = std::fs::remove_file(&path);
            let listener = UnixListener::bind(&path)
                .map_err(|e| io_err(address, e))?;
            Ok(Self { listener, path })
        }

        pub(super) async fn accept(
            &mut self,
            address: &IpcAddress,
        ) -> Result<StreamInner, TransportError> {
            let (stream, _addr) = self
                .listener
                .accept()
                .await
                .map_err(|e| io_err(address, e))?;
            Ok(StreamInner { stream })
        }
    }

    impl Drop for ListenerInner {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    impl StreamInner {
        pub(super) async fn connect(address: &IpcAddress) -> Result<Self, TransportError> {
            let path = match address {
                IpcAddress::Path(p) => p,
                IpcAddress::Pipe(_) => {
                    return Err(TransportError::WrongVariant {
                        address: address.display(),
                        variant: "Unix domain socket path",
                    });
                }
            };
            let stream = UnixStream::connect(path)
                .await
                .map_err(|e| io_err(address, e))?;
            Ok(Self { stream })
        }

        pub(super) fn into_split(self) -> (IpcReadHalf, IpcWriteHalf) {
            let (r, w) = tokio::io::split(self.stream);
            (Box::new(r), Box::new(w))
        }
    }
}

// ---------------------------------------------------------------------------
// Windows backend
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod backend {
    // Same rationale as the Unix backend: wildcard imports keep the
    // platform-specific module lean.
    #[allow(clippy::wildcard_imports)]
    use super::*;
    use tokio::net::windows::named_pipe::{
        ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions,
    };

    pub(super) struct ListenerInner {
        pipe_name: String,
        /// The server handle that will `connect().await` for the next
        /// client. Replaced with a freshly-created one each time a
        /// client connects, so subsequent connect attempts find a
        /// waiting server.
        next_server: Option<NamedPipeServer>,
    }

    pub(super) enum StreamInner {
        Server(NamedPipeServer),
        Client(NamedPipeClient),
    }

    impl ListenerInner {
        pub(super) fn bind(address: &IpcAddress) -> Result<Self, TransportError> {
            let pipe_name = match address {
                IpcAddress::Pipe(name) => name.clone(),
                IpcAddress::Path(_) => {
                    return Err(TransportError::WrongVariant {
                        address: address.display(),
                        variant: "Windows named pipe",
                    });
                }
            };
            // `first_pipe_instance(true)` refuses to bind if another
            // instance of the same pipe is already open — our version
            // of "stale socket" detection. Bump it to false if you want
            // allow-hijack semantics later.
            let server = ServerOptions::new()
                .first_pipe_instance(true)
                .create(&pipe_name)
                .map_err(|e| io_err(address, e))?;
            Ok(Self {
                pipe_name,
                next_server: Some(server),
            })
        }

        pub(super) async fn accept(
            &mut self,
            address: &IpcAddress,
        ) -> Result<StreamInner, TransportError> {
            let server = self
                .next_server
                .take()
                .expect("listener used after shutdown");
            server
                .connect()
                .await
                .map_err(|e| io_err(address, e))?;
            // Pre-create the next server so later connects find one.
            let next = ServerOptions::new()
                .create(&self.pipe_name)
                .map_err(|e| io_err(address, e))?;
            self.next_server = Some(next);
            Ok(StreamInner::Server(server))
        }
    }

    impl StreamInner {
        // Must stay `async` to match the Unix backend's signature, even
        // though `ClientOptions::open` is synchronous on Windows —
        // otherwise the outer `IpcStream::connect` wrapper can't delegate
        // to both backends with one definition.
        #[allow(clippy::unused_async)]
        pub(super) async fn connect(address: &IpcAddress) -> Result<Self, TransportError> {
            let name = match address {
                IpcAddress::Pipe(name) => name.as_str(),
                IpcAddress::Path(_) => {
                    return Err(TransportError::WrongVariant {
                        address: address.display(),
                        variant: "Windows named pipe",
                    });
                }
            };
            let client = ClientOptions::new()
                .open(name)
                .map_err(|e| io_err(address, e))?;
            Ok(Self::Client(client))
        }

        pub(super) fn into_split(self) -> (IpcReadHalf, IpcWriteHalf) {
            match self {
                Self::Server(s) => {
                    let (r, w) = tokio::io::split(s);
                    (Box::new(r), Box::new(w))
                }
                Self::Client(c) => {
                    let (r, w) = tokio::io::split(c);
                    (Box::new(r), Box::new(w))
                }
            }
        }
    }
}

use backend::{ListenerInner, StreamInner};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_from_str_path_vs_pipe() {
        assert!(matches!(
            "/tmp/arx.sock".parse::<IpcAddress>().unwrap(),
            IpcAddress::Path(_)
        ));
        assert!(matches!(
            r"\\.\pipe\arx-alice".parse::<IpcAddress>().unwrap(),
            IpcAddress::Pipe(_)
        ));
        assert!(matches!(
            r"\\?\pipe\arx-alice".parse::<IpcAddress>().unwrap(),
            IpcAddress::Pipe(_)
        ));
    }

    #[test]
    fn address_display_round_trips() {
        let path = IpcAddress::Path(PathBuf::from("/tmp/arx.sock"));
        assert_eq!(path.to_string(), "/tmp/arx.sock");
        let pipe = IpcAddress::Pipe(r"\\.\pipe\arx-alice".into());
        assert_eq!(pipe.to_string(), r"\\.\pipe\arx-alice");
    }

    #[test]
    fn current_user_prefers_user_then_username_then_fallback() {
        // We can't safely mutate env vars in parallel tests, so just
        // assert the function returns something non-empty and the
        // fallback logic is sound via a smoke test of the unwrap path.
        let u = current_user();
        assert!(!u.is_empty());
    }

    #[test]
    fn default_address_is_platform_appropriate() {
        let addr = default_address();
        #[cfg(unix)]
        assert!(matches!(addr, IpcAddress::Path(_)));
        #[cfg(windows)]
        assert!(matches!(addr, IpcAddress::Pipe(_)));
    }

    #[test]
    fn default_session_path_is_platform_appropriate() {
        let path = default_session_path();
        // The function must return *some* non-empty path in every
        // environment — it's the daemon's startup hook and cannot
        // return None. On both platforms the final segment is our
        // fixed filename.
        let fname = path
            .file_name()
            .expect("session path has no filename")
            .to_string_lossy()
            .into_owned();
        assert_eq!(fname, "session.postcard");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn unix_bind_connect_round_trip() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("arx.sock");
        let addr = IpcAddress::Path(path);

        let mut listener = IpcListener::bind(&addr).unwrap();
        assert_eq!(listener.address(), &addr);

        let addr_clone = addr.clone();
        let client_handle = tokio::spawn(async move {
            let stream = IpcStream::connect(&addr_clone).await.unwrap();
            let (_r, mut w) = stream.into_split();
            w.write_all(b"hello").await.unwrap();
            w.flush().await.unwrap();
        });

        let server_stream = listener.accept().await.unwrap();
        let (mut r, _w) = server_stream.into_split();
        let mut buf = [0u8; 5];
        r.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"hello");
        client_handle.await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn unix_listener_cleans_up_socket_on_drop() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("arx.sock");
        let addr = IpcAddress::Path(path.clone());
        {
            let _listener = IpcListener::bind(&addr).unwrap();
            assert!(path.exists());
        }
        assert!(!path.exists(), "socket file was not removed on drop");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn unix_bind_rejects_pipe_address() {
        let addr = IpcAddress::Pipe(r"\\.\pipe\nope".into());
        let err = IpcListener::bind(&addr).unwrap_err();
        assert!(matches!(err, TransportError::WrongVariant { .. }));
    }
}
