use async_trait::async_trait;
use tokio::net::UnixStream;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio_util::codec::{FramedRead, FramedWrite, LinesCodec};
use crate::jsonrpc_forwarder::JsonRPCForwarder;

pub struct UnixJsonRPCForwarder {
}

#[async_trait(?Send)]
impl JsonRPCForwarder<OwnedReadHalf, OwnedWriteHalf> for UnixJsonRPCForwarder  {
    async fn connect_stream(&self) -> anyhow::Result<(
        FramedRead<ReadHalf<NamedPipeClient>, LinesCodec>,
        FramedWrite<WriteHalf<NamedPipeClient>, LinesCodec>,
    )> {
        // Unix domain socket approach
        let stream = UnixStream::connect(&self.socket_path).await?;
        let (read_half, write_half) = stream.into_split();
        let reader = FramedRead::new(read_half, LinesCodec::new());
        let writer = FramedWrite::new(write_half, LinesCodec::new());

        Ok((reader, writer))
    }
}