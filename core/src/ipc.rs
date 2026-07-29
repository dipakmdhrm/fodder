//! Daemon↔viewer IPC over the `$XDG_RUNTIME_DIR` Unix socket.
//!
//! Framing is a 4-byte big-endian length prefix followed by a `serde_json`
//! body. Self-describing, size-tolerant, and free of delimiter-in-payload
//! hazards. The same socket also acts as the single-instance guard.

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Upper bound on a single frame (8 MiB) — guards against a corrupt length
/// header triggering a huge allocation.
const MAX_FRAME_LEN: u32 = 8 * 1024 * 1024;

/// Messages exchanged between the daemon and the viewer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IpcMessage {
    // --- liveness / handshake ---
    /// Stale-socket probe sent by a would-be second daemon.
    Ping,
    /// Reply to [`IpcMessage::Ping`] from a live daemon.
    Pong,
    /// The viewer registers itself with the daemon on startup.
    ViewerHello,
    /// The viewer is shutting down.
    ViewerClosing,

    // --- viewer -> daemon ---
    /// Poll now — one feed, or all when `feed_id` is `None`.
    RefreshNow {
        feed_id: Option<i64>,
    },
    /// A resolved subscription the daemon should persist and start polling.
    SubscribeResolved {
        feed_url: String,
        title: String,
    },
    /// The config file changed; the daemon should reload it.
    ReloadConfig,
    /// The viewer reports what it's currently showing, so the daemon can restore
    /// it on the next open. `article_id`/`feed_id` are `None` when nothing is open.
    ReadingState {
        feed_id: Option<i64>,
        article_id: Option<i64>,
        webkit: bool,
    },

    // --- daemon -> viewer ---
    /// Raise/show the viewer window.
    OpenViewer,
    /// Navigate the viewer to a feed (and optionally an article).
    OpenAt {
        feed_id: i64,
        article_id: Option<i64>,
    },
    /// The feed set or article data changed; the viewer should reload from DB.
    FeedsChanged,

    // --- generic replies ---
    Ack,
    Error(String),
}

/// Write one framed message.
pub async fn write_msg<W: AsyncWrite + Unpin>(w: &mut W, msg: &IpcMessage) -> std::io::Result<()> {
    let body = serde_json::to_vec(msg)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    if body.len() as u64 > MAX_FRAME_LEN as u64 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "message exceeds max frame length",
        ));
    }
    w.write_all(&(body.len() as u32).to_be_bytes()).await?;
    w.write_all(&body).await?;
    w.flush().await?;
    Ok(())
}

/// Read one framed message. Returns `Ok(None)` on a clean EOF at a frame
/// boundary (peer closed the connection).
pub async fn read_msg<R: AsyncRead + Unpin>(r: &mut R) -> std::io::Result<Option<IpcMessage>> {
    let mut len_buf = [0u8; 4];
    match r.read_exact(&mut len_buf).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_be_bytes(len_buf);
    if len > MAX_FRAME_LEN {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "frame length exceeds max",
        ));
    }
    let mut body = vec![0u8; len as usize];
    r.read_exact(&mut body).await?;
    let msg = serde_json::from_slice(&body)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok(Some(msg))
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn roundtrip(msg: IpcMessage) -> IpcMessage {
        // Buffer must exceed the largest framed message, since we write the
        // whole frame before reading it back on the same task.
        let (mut a, mut b) = tokio::io::duplex(4096);
        write_msg(&mut a, &msg).await.unwrap();
        read_msg(&mut b).await.unwrap().unwrap()
    }

    #[tokio::test]
    async fn roundtrip_all_variants() {
        let msgs = vec![
            IpcMessage::Ping,
            IpcMessage::Pong,
            IpcMessage::ViewerHello,
            IpcMessage::ViewerClosing,
            IpcMessage::RefreshNow { feed_id: Some(7) },
            IpcMessage::RefreshNow { feed_id: None },
            IpcMessage::SubscribeResolved {
                feed_url: "https://e.com/f".into(),
                title: "E".into(),
            },
            IpcMessage::ReloadConfig,
            IpcMessage::ReadingState {
                feed_id: Some(2),
                article_id: Some(9),
                webkit: true,
            },
            IpcMessage::OpenViewer,
            IpcMessage::OpenAt {
                feed_id: 3,
                article_id: Some(9),
            },
            IpcMessage::FeedsChanged,
            IpcMessage::Ack,
            IpcMessage::Error("boom".into()),
        ];
        for m in msgs {
            assert_eq!(roundtrip(m.clone()).await, m);
        }
    }

    #[tokio::test]
    async fn partial_frame_read_still_decodes() {
        // Write the frame byte-by-byte across the duplex to exercise the
        // read_exact loop reassembling a split header/body.
        let msg = IpcMessage::OpenAt {
            feed_id: 42,
            article_id: None,
        };
        // Build the framed bytes manually so we can dribble them one byte at a
        // time and prove read_exact reassembles a split header/body.
        let mut buf = Vec::new();
        let body = serde_json::to_vec(&msg).unwrap();
        buf.extend_from_slice(&(body.len() as u32).to_be_bytes());
        buf.extend_from_slice(&body);

        let (mut a, mut b) = tokio::io::duplex(1);
        let writer = tokio::spawn(async move {
            for byte in buf {
                a.write_all(&[byte]).await.unwrap();
            }
        });
        let got = read_msg(&mut b).await.unwrap().unwrap();
        writer.await.unwrap();
        assert_eq!(got, msg);
    }

    #[tokio::test]
    async fn clean_eof_returns_none() {
        let (a, mut b) = tokio::io::duplex(64);
        drop(a);
        assert!(read_msg(&mut b).await.unwrap().is_none());
    }
}
