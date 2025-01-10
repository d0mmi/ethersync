use std::path::PathBuf;
use async_trait::async_trait;
use tokio::net::UnixStream;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio_util::codec::{FramedRead, FramedWrite, LinesCodec};
use crate::jsonrpc_forwarder::JsonRPCForwarder;

pub struct UnixJsonRPCForwarder {
    pub socket_path: PathBuf,
}

#[async_trait(?Send)]
impl JsonRPCForwarder<OwnedReadHalf, OwnedWriteHalf> for UnixJsonRPCForwarder {
    async fn connect_stream(&self) -> anyhow::Result<(
        FramedRead<OwnedReadHalf, LinesCodec>,
        FramedWrite<OwnedWriteHalf, LinesCodec>,
    )> {
        // Unix domain socket approach
        let stream = UnixStream::connect(&self.socket_path).await?;
        let (read_half, write_half) = stream.into_split();
        let reader = FramedRead::new(read_half, LinesCodec::new());
        let writer = FramedWrite::new(write_half, LinesCodec::new());
        Ok((reader, writer))
    }
}