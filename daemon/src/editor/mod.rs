//! This module is all about daemon to editor communication.
#[cfg(windows)]
pub mod windows;
#[cfg(unix)]
pub mod unix;
use crate::types::EditorProtocolObject;
use anyhow::Result;
use std::{
    path::{PathBuf},
};
use tokio::io::{WriteHalf};
use tokio::net::windows::named_pipe::NamedPipeServer;
use tokio_util::{
    bytes::BytesMut,
    codec::Encoder,
};
use tokio_util::codec::FramedWrite;
use crate::daemon::DocumentActorHandle;

pub type EditorId = usize;

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
#[cfg(windows)]
pub type EditorWriter = FramedWrite<WriteHalf<NamedPipeServer>, EditorProtocolCodec>;
#[cfg(unix)]
pub type EditorWriter = FramedWrite<WriteHalf<UnixStream>, EditorProtocolCodec>;
pub trait Editor {
    fn get_socket_path(&self) -> PathBuf;
    fn make_editor_connection(&self, document_handle: DocumentActorHandle);
}