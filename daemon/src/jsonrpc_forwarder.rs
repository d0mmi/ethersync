//! Provides a way to write/read a socket (or TCP on Windows) through stdin, (un)packing content-length encoding.
//!
//! The idea is that a daemon process communicates via newline-separated jsonrpc messages,
//! whereas LSP expects HTTP-like Base Protocol with content-length headers.
//!
//! This forwarder:
//! - Takes jsonrpc from a socket/TCP (daemon) and wraps it into content-length-encoded data to stdout
//! - Takes content-length-encoded data from stdin (as sent by an LSP client) and writes it
//!   "unpacked" to the socket/TCP.

use anyhow::Result;
use futures::{SinkExt, StreamExt};
use std::path::Path;
use tokio::io::{BufReader, BufWriter};
#[cfg(unix)]
use tokio::net::UnixStream;
#[cfg(unix)]
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf, UnixStream};
#[cfg(windows)]
use tokio::net::windows::named_pipe::NamedPipeServer; // todo maybe named pipe is wrong and tcp was correct?
#[cfg(windows)]
use tokio::net::windows::named_pipe::NamedPipeClient;
#[cfg(windows)]
use tokio::net::windows::named_pipe::ClientOptions;
use tokio::net::windows::named_pipe::PipeMode;
use tokio_util::bytes::{Buf, BytesMut};
use tokio_util::codec::{Decoder, Encoder, FramedRead, FramedWrite, LinesCodec};
use tracing::info;

/// **Public** function remains the same name/signature.
/// It just calls our platform-specific `connect_stream`.
pub async fn connection(socket_path: &Path) -> Result<()> {
    // On Unix, connect to the Unix domain socket. On Windows, connect via TCP.
    let (mut socket_read, mut socket_write) = connect_stream(socket_path).await?;
    // Construct stdin/stdout objects, which send/receive messages with a Content-Length header.
    let mut stdin = FramedRead::new(BufReader::new(tokio::io::stdin()), ContentLengthCodec);
    let mut stdout = FramedWrite::new(BufWriter::new(tokio::io::stdout()), ContentLengthCodec);

    // Spawn a task that reads from the socket and forwards to stdout.
    tokio::spawn(async move {
        while let Some(Ok(message)) = socket_read.next().await {
            stdout
                .send(message)
                .await
                .expect("Failed to write to stdout");
        }
        // If the socket/TCP is closed, exit.
        std::process::exit(0);
    });

    // Main thread: read from stdin and write to the socket/TCP.
    while let Some(Ok(message)) = stdin.next().await {
        socket_write.send(message).await?;
    }

    // If stdin is closed, exit.
    std::process::exit(0);
}

/// Connect to the socket or TCP stream and return framed `LinesCodec` readers/writers.
#[cfg(unix)]
async fn connect_stream(
    socket_path: &Path,
) -> Result<(FramedRead<OwnedReadHalf, LinesCodec>, FramedWrite<OwnedWriteHalf, LinesCodec>)> {
    // Unix domain socket approach
    let stream = UnixStream::connect(socket_path).await?;
    let (read_half, write_half) = stream.into_split();
    let reader = FramedRead::new(read_half, LinesCodec::new());
    let writer = FramedWrite::new(write_half, LinesCodec::new());
    Ok((reader, writer))
}

/// On Windows, we just do TCP to 127.0.0.1:9000 (or any other port you want).
#[cfg(windows)]
async fn connect_stream(
    socket_path: &Path,
) -> Result<(FramedRead<tokio::io::ReadHalf<NamedPipeClient>, LinesCodec>, FramedWrite<tokio::io::WriteHalf<NamedPipeClient>, LinesCodec>)> {
    // Convert the Path to a UTF-8 string and prepend the named pipe prefix
    let pipe_name = format!(r"\\.\pipe\{}", socket_path.to_str().unwrap().split('\\').last().unwrap());
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

/// Codec that wraps messages in `Content-Length:` headers (LSP style).
struct ContentLengthCodec;

impl Encoder<String> for ContentLengthCodec {
    type Error = std::io::Error;

    fn encode(&mut self, item: String, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let content_length = item.len();
        dst.extend_from_slice(format!("Content-Length: {}\r\n\r\n", content_length).as_bytes());
        dst.extend_from_slice(item.as_bytes());
        Ok(())
    }
}

impl Decoder for ContentLengthCodec {
    type Item = String;
    type Error = anyhow::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        // Find the position of the "Content-Length: " header.
        let c = b"Content-Length: ";
        let start_of_header = match src.windows(c.len()).position(|window| window == c) {
            Some(pos) => pos,
            None => return Ok(None),
        };

        // Find the end-of-headers marker (\r\n\r\n or \n\n).
        let (end_of_line, end_of_line_bytes) = match src[start_of_header + c.len()..]
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
        {
            Some(pos) => (pos, 4),
            None => match src[start_of_header + c.len()..]
                .windows(2)
                .position(|window| window == b"\n\n")
            {
                Some(pos) => (pos, 2),
                None => return Ok(None),
            },
        };

        // Parse the Content-Length number.
        let content_length_str = &src[start_of_header + c.len()
            ..start_of_header + c.len() + end_of_line];
        let content_length: usize = std::str::from_utf8(content_length_str)?.parse()?;

        // The start of the actual JSON content.
        let content_start = start_of_header + c.len() + end_of_line + end_of_line_bytes;

        // If we haven't received enough bytes yet, wait.
        if src.len() < content_start + content_length {
            return Ok(None);
        }

        // Advance the buffer past the headers.
        src.advance(content_start);

        // Split out the JSON text.
        let content = src.split_to(content_length);
        let text = std::str::from_utf8(&content)?.to_string();

        Ok(Some(text))
    }
}
