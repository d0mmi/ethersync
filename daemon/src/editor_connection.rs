use std::{
    collections::HashMap,
    env,
    path::{Path, PathBuf},
};

use anyhow::{bail, Context};
use futures::SinkExt;
use tracing::debug;

use crate::{
    editor::EditorHandle,
    ot::OTServer,
    sandbox,
    types::{
        EditorProtocolMessageError, EditorProtocolMessageFromEditor, EditorProtocolMessageToEditor,
        EditorProtocolObject, InsideMessage, RevisionedEditorTextDelta,
    },
};

/// Represents a connection to an editor. Knows how to communicate with it, and handles the OT.
pub struct EditorConnection {
    id: String,
    handle: EditorHandle,
    // TODO: Feels a bit duplicated here?
    base_dir: PathBuf,
    /// There's one OTServer per open buffer.
    ot_servers: HashMap<String, OTServer>,
}

impl EditorConnection {
    pub fn new(id: String, handle: EditorHandle, base_dir: &Path) -> Self {
        Self {
            id,
            handle,
            base_dir: base_dir.to_owned(),
            ot_servers: HashMap::new(),
        }
    }

    pub fn owns(&self, file_path: &str) -> bool {
        self.ot_servers.contains_key(file_path)
    }

    pub async fn message_from_inside(&mut self, message: &InsideMessage) -> anyhow::Result<()> {
        match message {
            InsideMessage::Edit { file_path, delta } => {
                if let Some(ot_server) = self.ot_servers.get_mut(file_path) {
                    debug!("Applying incoming CRDT patch for {file_path}");
                    let rev_text_delta_for_editor = ot_server.apply_crdt_change(delta);
                    self.send_to_editor(rev_text_delta_for_editor, file_path)
                        .await?;
                }
            }
            InsideMessage::Cursor {
                file_path,
                ranges,
                name,
                cursor_id,
            } => {
                let uri = format!("file://{}", self.absolute_path_for_file_path(file_path));
                let message = EditorProtocolMessageToEditor::Cursor {
                    name: name.clone(),
                    userid: cursor_id.clone(),
                    uri,
                    ranges: ranges.clone(),
                };
                self.send_to_editor_client(EditorProtocolObject::Request(message))
                    .await?;
            }
            _ => {
                debug!("Ignoring message from inside: {:#?}", message);
            }
        }
        Ok(())
    }

    pub async fn message_from_outside(
        &mut self,
        message: &EditorProtocolMessageFromEditor,
    ) -> Result<InsideMessage, EditorProtocolMessageError> {
        fn anyhow_err_to_protocol_err(error: anyhow::Error) -> EditorProtocolMessageError {
            EditorProtocolMessageError {
                code: -1, // TODO: Should the error codes differ per error?
                message: error.to_string(),
                data: None,
            }
        }

        match message {
            EditorProtocolMessageFromEditor::Open { uri } => {
                let file_path = self
                    .file_path_for_uri(uri)
                    .map_err(anyhow_err_to_protocol_err)?;

                debug!("Got an 'open' message for {file_path}");
                let absolute_file_path = self.absolute_path_for_file_path(&file_path);
                let absolute_file_path = Path::new(&absolute_file_path);
                if !sandbox::exists(&self.base_dir, absolute_file_path)
                    .map_err(anyhow_err_to_protocol_err)?
                {
                    // Creating nonexisting files allows us to traverse this file for whether it's
                    // ignored, which is needed to even be allowed to open it.
                    sandbox::write_file(&self.base_dir, absolute_file_path, b"")
                        .map_err(anyhow_err_to_protocol_err)?;
                }

                // We only want to process these messages for files that are not ignored.
                if crate::ignore::is_ignored(&self.base_dir, absolute_file_path) {
                    return Err(EditorProtocolMessageError {
                        code: -1,
                        message: format!("File '{absolute_file_path:?}' is ignored"),
                        data: Some("This file should not be shared with other peers".into()),
                    });
                }

                let bytes = sandbox::read_file(&self.base_dir, absolute_file_path)
                    .map_err(anyhow_err_to_protocol_err)?;
                let text = String::from_utf8(bytes)
                    .context("Failed to convert bytes to string")
                    .map_err(anyhow_err_to_protocol_err)?;

                let ot_server = OTServer::new(text);
                self.ot_servers.insert(file_path.clone(), ot_server);

                Ok(InsideMessage::Open { file_path })
            }
            EditorProtocolMessageFromEditor::Close { uri } => {
                let file_path = self
                    .file_path_for_uri(uri)
                    .map_err(anyhow_err_to_protocol_err)?;
                debug!("Got a 'close' message for {file_path}");
                self.ot_servers.remove(&file_path);

                Ok(InsideMessage::Close { file_path })
            }
            EditorProtocolMessageFromEditor::Edit {
                delta: rev_delta,
                uri,
            } => {
                debug!("Handling RevDelta from editor: {:#?}", rev_delta);
                let file_path = self
                    .file_path_for_uri(uri)
                    .map_err(anyhow_err_to_protocol_err)?;
                if self.ot_servers.get_mut(&file_path).is_none() {
                    return Err(EditorProtocolMessageError {
                        code: -1,
                        message: "File not found".into(),
                        data: Some(
                            "Please stop sending edits for this file or 'open' it before.".into(),
                        ),
                    });
                }

                let ot_server = self
                    .ot_servers
                    .get_mut(&file_path)
                    .expect("Could not find OT server.");
                let (delta_for_crdt, rev_deltas_for_editor) =
                    ot_server.apply_editor_operation(rev_delta.clone());

                for rev_delta_for_editor in rev_deltas_for_editor {
                    self.send_to_editor(rev_delta_for_editor, &file_path)
                        .await
                        .map_err(anyhow_err_to_protocol_err)?;
                }

                Ok(InsideMessage::Edit {
                    file_path,
                    delta: delta_for_crdt,
                })
            }
            EditorProtocolMessageFromEditor::Cursor { uri, ranges } => {
                let file_path = self
                    .file_path_for_uri(uri)
                    .map_err(anyhow_err_to_protocol_err)?;
                Ok(InsideMessage::Cursor {
                    cursor_id: self.id.clone(),
                    name: env::var("USER").ok(),
                    file_path,
                    ranges: ranges.clone(),
                })
            }
        }
    }

    async fn send_to_editor(
        &mut self,
        rev_delta: RevisionedEditorTextDelta,
        file_path: &str,
    ) -> anyhow::Result<()> {
        let message = EditorProtocolMessageToEditor::Edit {
            uri: format!("file://{}", self.absolute_path_for_file_path(file_path)),
            delta: rev_delta,
        };
        self.send_to_editor_client(EditorProtocolObject::Request(message))
            .await
    }

    // TODO: Make private in the end.
    pub async fn send_to_editor_client(
        &mut self,
        message: EditorProtocolObject,
    ) -> anyhow::Result<()> {
        self.handle
            .send(message)
            .await
            .context("Failed to send message to editor. It probably has disconnected.")
        // TODO: Remove on error in daemon?
    }

    fn absolute_path_for_file_path(&self, file_path: &str) -> String {
        format!("{}/{}", self.base_dir.display(), file_path)
    }

    fn file_path_for_uri(&self, uri: &str) -> anyhow::Result<String> {
        // If uri starts with "file://", we remove it.
        let absolute_path = uri.strip_prefix("file://").unwrap_or(uri);

        // Check that it's an absolute path.
        if !absolute_path.starts_with('/') {
            bail!("Path '{absolute_path}' is not an absolute file:// URI");
        }

        let base_dir_string = self.base_dir.display().to_string() + "/";

        Ok(absolute_path
            .strip_prefix(&base_dir_string)
            .with_context(|| {
                format!("Path '{absolute_path}' is not within base dir '{base_dir_string}'")
            })?
            .to_string())
    }
}
