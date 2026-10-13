//! stdio ⇄ `mcp.sock`: what an MCP client launches (`yuki mcp`). It presents the
//! token, then pipes JSON-RPC lines both ways until either side closes.

use std::{io, path::Path};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

pub async fn run(socket: &Path, token: &str) -> io::Result<()> {
    let stream = UnixStream::connect(socket).await.map_err(|e| {
        io::Error::new(
            e.kind(),
            format!("is Yuki running? no socket at {}: {e}", socket.display()),
        )
    })?;
    let (read, mut write) = stream.into_split();
    let mut read = BufReader::new(read);
    write.write_all(format!("{token}\n").as_bytes()).await?;
    let mut reply = String::new();
    read.read_line(&mut reply).await?;
    if reply.trim() != "OK" {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("Yuki refused the token: {}", reply.trim()),
        ));
    }
    let (mut stdin, mut stdout) = (tokio::io::stdin(), tokio::io::stdout());
    let up = tokio::io::copy(&mut stdin, &mut write);
    let down = tokio::io::copy(&mut read, &mut stdout);
    tokio::select! {
        r = up => r.map(drop),
        r = down => r.map(drop),
    }
}
