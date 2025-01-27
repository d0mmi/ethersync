use std::io;
use std::path::{PathBuf};
use futures::StreamExt;
use tokio::net::windows::named_pipe::{NamedPipeServer, PipeMode, ServerOptions};
use tokio_util::{codec::{FramedRead, FramedWrite, LinesCodec}};
use tracing::info;
use crate::daemon::{DocMessage, DocumentActorHandle};
use crate::editor::{Editor, EditorId, EditorProtocolCodec, EditorWriter};

pub struct EditorWindows;

impl Editor for EditorWindows {
    fn get_socket_path(&self) -> PathBuf {
        PathBuf::from("ethersync_pipe")
    }

    fn make_editor_connection(&self, document_handle: DocumentActorHandle) {
        let socket_path = self.get_socket_path();
        tokio::spawn(async move {
            accept_editor_loop(socket_path, document_handle).await.expect("Failed to make editor connection!");
        });
    }
}

async fn accept_editor_loop(
    socket_path: PathBuf,
    document_handle: DocumentActorHandle,
) -> io::Result<()> {
    let pipe_name = format!(
        r"\\.\pipe\{}",
        socket_path.to_str().unwrap().split('\\').last().unwrap()
    );
    loop {
        let mut server_options = ServerOptions::new();
        server_options.pipe_mode(PipeMode::Byte);
        let pipe: NamedPipeServer = server_options.create(&pipe_name)?;
        info!("Listening for connections on named pipe: {}", pipe_name);

        // Wait asynchronously for a client to connect
        pipe.connect().await?;
        info!("Client connected!");
        let id = document_handle.next_editor_id();
        spawn_editor_connection(pipe, document_handle.clone(), id);
    }
}

fn spawn_editor_connection(
    stream: NamedPipeServer,
    document_handle: DocumentActorHandle,
    editor_id: EditorId,
) {
    tokio::spawn(async move {
        let (stream_read, stream_write) = tokio::io::split(stream);
        let mut reader = FramedRead::new(stream_read, LinesCodec::new());
        let writer: EditorWriter = FramedWrite::new(stream_write, EditorProtocolCodec);

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