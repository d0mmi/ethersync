use std::path::PathBuf;
use async_trait::async_trait;
use tokio::io::{ReadHalf, WriteHalf};
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient, PipeMode};
use tokio_util::codec::{FramedRead, FramedWrite, LinesCodec};
use crate::jsonrpc_forwarder::JsonRPCForwarder;

pub struct WindowsJsonRPCForwarder {
    pub pipe_name: PathBuf,
}

#[async_trait(?Send)]
impl JsonRPCForwarder<ReadHalf<NamedPipeClient>, WriteHalf<NamedPipeClient>> for WindowsJsonRPCForwarder {
    async fn connect_stream(&self) -> anyhow::Result<(
        FramedRead<ReadHalf<NamedPipeClient>, LinesCodec>,
        FramedWrite<WriteHalf<NamedPipeClient>, LinesCodec>,
    )> {
        // Convert the Path to a UTF-8 string and prepend the named pipe prefix
        let pipe_name = format!(
            r"\\.\pipe\{}",
            self.pipe_name.to_str().unwrap().split('\\').last().unwrap()
        );
        // Attempt to create the client
        let mut client_options = ClientOptions::new();
        client_options.pipe_mode(PipeMode::Byte);
        let client = client_options.open(&pipe_name)?;

        // Split the named pipe into read and write halves
        let (read_half, write_half) = tokio::io::split(client);

        // Create FramedRead and FramedWrite for line-based codec
        let reader = FramedRead::new(read_half, LinesCodec::new());
        let writer = FramedWrite::new(write_half, LinesCodec::new());

        Ok((reader, writer))
    }
}