use crate::sandbox;
use anyhow::{bail, Context, Result};
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use futures::StreamExt;
use tokio::net::{UnixListener, UnixStream};
use tokio_util::codec::{FramedRead, FramedWrite, LinesCodec};
use tracing::info;
use crate::daemon::{DocMessage, DocumentActorHandle};
use crate::editor::{Editor, EditorId, EditorProtocolCodec};

pub struct EditorUnix {
  pub socket_name:PathBuf
}
impl Editor for EditorUnix {
    fn get_socket_path(&self) -> PathBuf {
        let socket_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| get_fallback_socket_dir());
        let socket_dir = Path::new(&socket_dir);
        if let Err(description) = is_valid_socket_name(&self.socket_name) {
            panic!("{}", description);
        }
        socket_dir.join(&self.socket_name)
    }

    /// # Panics
    ///
    /// Will panic if we fail to listen on the socket, or if we fail to accept an incoming connection.
     fn make_editor_connection(&self, document_handle: DocumentActorHandle) {
        // Make sure the parent directory of the socket is only accessible by the current user.
        let socket_path = self.get_socket_path();
        if let Err(description) = is_user_readable_only(Path::new(&socket_path)) {
            panic!("{}", description);
        }

        // Using the sandbox method here is technically unnecessary,
        // but we want to really run all path operations through the sandbox module.
        if sandbox::exists(Path::new("/"), Path::new(&socket_path))
            .expect("Failed to check existence of path")
        {
            sandbox::remove_file(Path::new("/"), &socket_path).expect("Could not remove socket");
        }

        tokio::spawn(async move {
            accept_editor_loop(&socket_path, document_handle).await.expect("Failed to make editor connection!");
        });
    }
}

fn get_fallback_socket_dir() -> String {
    let socket_dir = format!(
        "/tmp/ethersync-{}",
        std::env::var("USER").expect("$USER should be set")
    );
    if !fs::exists(&socket_dir).expect("Should be able to test for existence of directory in /tmp")
    {
        fs::create_dir(&socket_dir).expect("Should be able to create a directory in /tmp");
        let permissions = fs::Permissions::from_mode(0o700);
        fs::set_permissions(&socket_dir, permissions)
            .expect("Should be able to set permissions for a directory we just created");
    }
    socket_dir
}


fn is_valid_socket_name(socket_name: &Path) -> Result<()> {
    if socket_name.components().count() != 1 {
        bail!("The socket name must be a single path component");
    }
    if let std::path::Component::Normal(_) = socket_name
        .components()
        .next()
        .expect("The component count of socket_name was previously checked to be non-empty")
    {
        // All good :)
    } else {
        bail!("The socket name must be a plain filename");
    }
    Ok(())
}


fn is_user_readable_only(socket_path: &Path) -> Result<()> {
    let parent_dir = socket_path
        .parent()
        .context("The socket path should not be the root directory")?;
    let current_permissions = fs::metadata(parent_dir)
        .context("Expected to have access to metadata of the socket path's parent")?
        .permissions()
        .mode();
    // Group and others should not have any permissions.
    let allowed_permissions = 0o77700u32;
    if current_permissions | allowed_permissions != allowed_permissions {
        bail!("For security reasons, the parent directory of the socket must only be accessible by the current user");
    }
    Ok(())
}


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
        info!("Client #{editor_id} connected");

        while let Some(Ok(line)) = reader.next().await {
            document_handle
                .send_message(DocMessage::FromEditor(editor_id, line))
                .await;
        }

        document_handle
            .send_message(DocMessage::CloseEditorConnection(editor_id))
            .await;
        info!("Client #{editor_id} disconnected");
    });
}