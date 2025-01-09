//! This module is all about daemon to editor communication.
use crate::daemon::{DocMessage, DocumentActorHandle};
use crate::sandbox;
use crate::types::EditorProtocolObject;
use anyhow::{bail, Context, Result};
use futures::StreamExt;
use std::{
    fs, io,
    path::{Path, PathBuf},
};
use tokio::io::WriteHalf;
use tokio_util::{
    bytes::BytesMut,
    codec::{Encoder, FramedRead, FramedWrite, LinesCodec},
};
use tracing::info;

// ------------------------------------------------------------------------------------
// Cross-platform items
// ------------------------------------------------------------------------------------

pub type EditorId = usize;

/// This is our codec to write EditorProtocolObject lines (the same on both OSes).
pub struct EditorProtocolCodec;

impl Encoder<EditorProtocolObject> for EditorProtocolCodec {
    type Error = anyhow::Error;

    fn encode(
        &mut self,
        item: EditorProtocolObject,
        dst: &mut BytesMut,
    ) -> Result<(), Self::Error> {
        let payload = item.to_jsonrpc()?;
        dst.extend_from_slice(format!("{payload}\n").as_bytes());
        Ok(())
    }
}

// ------------------------------------------------------------------------------------
// Unix-specific imports and definitions
// ------------------------------------------------------------------------------------
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use tokio::net::{UnixListener, UnixStream};

/// On Unix, EditorWriter = `FramedWrite<WriteHalf<UnixStream>>`.
#[cfg(unix)]
pub type EditorWriter = FramedWrite<WriteHalf<UnixStream>, EditorProtocolCodec>;

/// Provide a fallback socket directory in `/tmp/ethersync-<user>`.
#[cfg(unix)]
fn get_fallback_socket_dir() -> String {
    let socket_dir = format!(
        "/tmp/ethersync-{}",
        std::env::var("USER").expect("$USER should be set")
    );
    if !fs::exists(&socket_dir).expect("Failed to check existence in /tmp") {
        fs::create_dir(&socket_dir).expect("Failed to create directory in /tmp");
        let permissions = fs::Permissions::from_mode(0o700);
        fs::set_permissions(&socket_dir, permissions)
            .expect("Failed to set permissions for directory we just created");
    }
    socket_dir
}

#[cfg(unix)]
fn is_valid_socket_name(socket_name: &Path) -> Result<()> {
    if socket_name.components().count() != 1 {
        bail!("The socket name must be a single path component");
    }
    if let std::path::Component::Normal(_) = socket_name
        .components()
        .next()
        .expect("socket_name was non-empty")
    {
        // OK
    } else {
        bail!("The socket name must be a plain filename");
    }
    Ok(())
}

/// Return the path `$XDG_RUNTIME_DIR/<socket_name>` or a fallback in `/tmp/ethersync-<user>`.
#[cfg(unix)]
pub fn get_socket_path(socket_name: &Path) -> PathBuf {
    let socket_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| get_fallback_socket_dir());
    let socket_dir = Path::new(&socket_dir);
    if let Err(description) = is_valid_socket_name(socket_name) {
        panic!("{}", description);
    }
    socket_dir.join(socket_name)
}

/// On Unix, ensure the parent directory has `0o700` perms.
#[cfg(unix)]
fn is_user_readable_only(socket_path: &Path) -> Result<()> {
    let parent_dir = socket_path
        .parent()
        .context("The socket path should not be the root directory")?;
    let current_permissions = fs::metadata(parent_dir)
        .context("Expected to have metadata of parent")?
        .permissions()
        .mode();
    let allowed_permissions = 0o700;
    if current_permissions != allowed_permissions {
        bail!(
            "For security reasons, the socket's parent dir must only be accessible by the current user"
        );
    }
    Ok(())
}

/// This is the **outer** function on Unix. It returns `()` and calls the inner `accept_editor_loop`.
#[cfg(unix)]
pub async fn make_editor_connection(socket_path: PathBuf, document_handle: DocumentActorHandle) {
    // Check directory permissions
    if let Err(description) = is_user_readable_only(&socket_path) {
        panic!("{}", description);
    }

    // If stale socket file exists, remove it
    if sandbox::exists(Path::new("/"), &socket_path).expect("Failed to check socket existence") {
        sandbox::remove_file(Path::new("/"), &socket_path).expect("Could not remove socket file");
    }

    // Now bind and accept in a separate function that returns `Result`.
    let result = accept_editor_loop(&socket_path, document_handle).await;
    match result {
        Ok(()) => {}
        Err(err) => {
            panic!("Failed to make editor connection: {err}");
        }
    }
}

/// On Unix, the **inner** function does the real work, returning `Result<()>`.
#[cfg(unix)]
async fn accept_editor_loop(
    socket_path: &Path,
    document_handle: DocumentActorHandle,
) -> Result<(), io::Error> {
    let listener = UnixListener::bind(socket_path)?;
    info!("Listening on UNIX socket: {}", socket_path.display());

    loop {
        let (stream, _addr) = listener.accept().await?;
        let id = document_handle.next_editor_id();
        spawn_editor_connection(stream, document_handle.clone(), id);
    }
}

#[cfg(unix)]
fn spawn_editor_connection(
    stream: UnixStream,
    document_handle: DocumentActorHandle,
    editor_id: EditorId,
) {
    tokio::spawn(async move {
        let (stream_read, stream_write) = tokio::io::split(stream);
        let mut reader = FramedRead::new(stream_read, LinesCodec::new());
        let writer = FramedWrite::new(stream_write, EditorProtocolCodec);

        document_handle
            .send_message(DocMessage::NewEditorConnection(editor_id, writer))
            .await;
        info!("Client #{editor_id} connected (UNIX)");

        while let Some(Ok(line)) = reader.next().await {
            document_handle
                .send_message(DocMessage::FromEditor(editor_id, line))
                .await;
        }

        document_handle
            .send_message(DocMessage::CloseEditorConnection(editor_id))
            .await;
        info!("Client #{editor_id} disconnected (UNIX)");
    });
}

// ------------------------------------------------------------------------------------
// Windows-specific imports and definitions
// ------------------------------------------------------------------------------------
#[cfg(windows)]
use tokio::net::windows::named_pipe::NamedPipeServer;
use tokio::net::windows::named_pipe::PipeMode;
#[cfg(windows)]
use tokio::net::windows::named_pipe::ServerOptions;

/// On Windows, EditorWriter = `FramedWrite<WriteHalf<TcpStream>>`.
#[cfg(windows)]
pub type EditorWriter = FramedWrite<WriteHalf<NamedPipeServer>, EditorProtocolCodec>;

/// On Windows, `get_socket_path` might just return something in %TEMP%.
#[cfg(windows)]
pub fn get_socket_path(_socket_name: &Path) -> PathBuf {
    std::env::temp_dir().join("ethersync_win.sock")
}

/// On Windows, we skip the "mode = 0700" check.
#[cfg(windows)]
fn is_user_readable_only(_socket_path: &Path) -> Result<()> {
    Ok(())
}

/// **Outer** function on Windows, returns `()`, calls an inner function returning `Result`.
#[cfg(windows)]
pub async fn make_editor_connection(_socket_path: PathBuf, document_handle: DocumentActorHandle) {
    // For minimal parity with Unix code, do a no-op permission check:
    if let Err(description) = is_user_readable_only(&_socket_path) {
        panic!("{}", description);
    }
    // todo remove all file related stuff
    // Also, we skip removing any file, but to keep the same shape:
    if sandbox::exists(Path::new("/"), &_socket_path).unwrap_or(false) {
        sandbox::remove_file(Path::new("/"), &_socket_path).ok();
    }

    // Call the inner function that returns `Result`.
    let result = accept_editor_loop_win(_socket_path, document_handle).await;
    match result {
        Ok(()) => {}
        Err(err) => {
            panic!("Failed to make editor connection: {err}");
        }
    }
}

/// **Inner** function on Windows that returns `Result<()>`, so we can use `?`.
#[cfg(windows)]
async fn accept_editor_loop_win(
    socket_path: PathBuf,
    document_handle: DocumentActorHandle,
) -> std::io::Result<()> {
    // Listen on Pipe instead of a Unix socket
    let pipe_name = format!(
        r"\\.\pipe\{}",
        socket_path.to_str().unwrap().split('\\').last().unwrap()
    );
    loop {
        let mut server_options = ServerOptions::new();
        server_options.pipe_mode(PipeMode::Byte);
        let pipe: NamedPipeServer = server_options.create(&pipe_name)?;
        info!("Client is connecting on named pipe: {}", pipe_name);

        // Wait asynchronously for a client to connect
        pipe.connect().await?;
        info!("Client connected!");
        let id = document_handle.next_editor_id();
        spawn_editor_connection_win(pipe, document_handle.clone(), id);
    }
}

#[cfg(windows)]
fn spawn_editor_connection_win(
    stream: NamedPipeServer,
    document_handle: DocumentActorHandle,
    editor_id: EditorId,
) {
    tokio::spawn(async move {
        let (stream_read, stream_write) = tokio::io::split(stream);
        let mut reader = FramedRead::new(stream_read, LinesCodec::new());
        let writer = FramedWrite::new(stream_write, EditorProtocolCodec);

        document_handle
            .send_message(DocMessage::NewEditorConnection(editor_id, writer))
            .await;
        info!("Client #{editor_id} connected (Windows)");

        while let Some(Ok(line)) = reader.next().await {
            document_handle
                .send_message(DocMessage::FromEditor(editor_id, line))
                .await;
        }

        document_handle
            .send_message(DocMessage::CloseEditorConnection(editor_id))
            .await;
        info!("Client #{editor_id} disconnected (Windows)");
    });
}
