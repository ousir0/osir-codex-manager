//! Loopback-only CONNECT proxy for the Codex UI localization bootstrap.

use std::io;
use std::sync::OnceLock;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

const DEFAULT_PORT: u16 = 19443;
const SERVER_TUNNEL_URL: &str = "wss://app.osirclaw.com/i18n-tunnel";
const MAX_HEADER_BYTES: usize = 16 * 1024;
const MAX_TUNNEL_BYTES: usize = 16 * 1024 * 1024;
const TUNNEL_LIFETIME: Duration = Duration::from_secs(5 * 60);

static PROXY_PORT: OnceLock<Option<u16>> = OnceLock::new();

pub fn ensure_started() -> Option<u16> {
    *PROXY_PORT.get_or_init(|| {
        let listener = std::net::TcpListener::bind(("127.0.0.1", DEFAULT_PORT)).ok()?;
        listener.set_nonblocking(true).ok()?;
        let port = listener.local_addr().ok()?.port();
        std::thread::Builder::new()
            .name("osir-i18n-proxy".to_string())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build();
                match runtime {
                    Ok(runtime) => runtime.block_on(run(listener)),
                    Err(error) => log::error!("failed to start OSIR i18n proxy runtime: {error}"),
                }
            })
            .ok()?;
        log::info!("OSIR i18n loopback proxy listening on 127.0.0.1:{port}");
        Some(port)
    })
}

pub fn pac_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/proxy.pac")
}

fn is_pac_request(headers: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(headers) else {
        return false;
    };
    matches!(text.lines().next(), Some(line) if line.starts_with("GET /proxy.pac "))
}

fn pac_script(port: u16) -> String {
    format!(
        "function FindProxyForURL(url, host) {{ if (host.toLowerCase() === \"ab.chatgpt.com\") return \"PROXY 127.0.0.1:{port}; DIRECT\"; return \"DIRECT\"; }}"
    )
}

async fn run(listener: std::net::TcpListener) {
    let listener = match TcpListener::from_std(listener) {
        Ok(listener) => listener,
        Err(error) => {
            log::error!("failed to bind OSIR i18n proxy listener: {error}");
            return;
        }
    };
    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                tokio::spawn(async move {
                    if let Err(error) = handle(stream).await {
                        log::debug!("OSIR i18n proxy peer={peer:?} closed: {error}");
                    }
                });
            }
            Err(error) => log::warn!("OSIR i18n proxy accept failed: {error}"),
        }
    }
}

async fn read_headers(stream: &mut TcpStream) -> io::Result<Vec<u8>> {
    let mut data = Vec::with_capacity(1024);
    let mut chunk = [0_u8; 1024];
    while data.len() <= MAX_HEADER_BYTES {
        let count = stream.read(&mut chunk).await?;
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "proxy client closed",
            ));
        }
        data.extend_from_slice(&chunk[..count]);
        if data.windows(4).any(|window| window == b"\r\n\r\n") {
            return Ok(data);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "proxy headers too large",
    ))
}

fn is_allowed_connect(headers: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(headers) else {
        return false;
    };
    let Some(line) = text.lines().next() else {
        return false;
    };
    let mut fields = line.split_whitespace();
    fields.next() == Some("CONNECT")
        && fields.next() == Some("ab.chatgpt.com:443")
        && fields.next() == Some("HTTP/1.1")
}

async fn handle(mut client: TcpStream) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let headers = read_headers(&mut client).await?;
    if is_pac_request(&headers) {
        let body = pac_script(DEFAULT_PORT);
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/x-ns-proxy-autoconfig\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{}",
            body.len(), body
        );
        client.write_all(response.as_bytes()).await?;
        return Ok(());
    }
    if !is_allowed_connect(&headers) {
        client
            .write_all(b"HTTP/1.1 403 Forbidden\r\nConnection: close\r\n\r\n")
            .await?;
        return Ok(());
    }

    let _ = rustls::crypto::ring::default_provider().install_default();
    let (socket, _) =
        tokio::time::timeout(TUNNEL_LIFETIME, connect_async(SERVER_TUNNEL_URL)).await??;
    let (mut ws_write, mut ws_read) = socket.split();
    match tokio::time::timeout(Duration::from_secs(10), ws_read.next()).await? {
        Some(Ok(Message::Text(text))) if text == "ready" => {}
        _ => return Err("OSIR tunnel did not become ready".into()),
    }
    client
        .write_all(b"HTTP/1.1 200 Connection Established\r\nConnection: keep-alive\r\n\r\n")
        .await?;
    let (mut client_read, mut client_write) = client.into_split();
    let upload = async {
        let mut total = 0usize;
        let mut buffer = [0_u8; 16 * 1024];
        while let Ok(count) = client_read.read(&mut buffer).await {
            if count == 0 {
                break;
            }
            total += count;
            if total > MAX_TUNNEL_BYTES {
                break;
            }
            ws_write
                .send(Message::Binary(buffer[..count].to_vec().into()))
                .await?;
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    };
    let download = async {
        let mut total = 0usize;
        while let Some(message) = ws_read.next().await {
            match message? {
                Message::Binary(bytes) => {
                    total += bytes.len();
                    if total > MAX_TUNNEL_BYTES {
                        break;
                    }
                    client_write.write_all(&bytes).await?;
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    };
    tokio::time::timeout(TUNNEL_LIFETIME, async {
        tokio::select! { result = upload => result, result = download => result }
    })
    .await??;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{is_allowed_connect, is_pac_request, pac_url};

    #[test]
    fn only_the_i18n_connect_target_is_allowed() {
        assert!(is_allowed_connect(
            b"CONNECT ab.chatgpt.com:443 HTTP/1.1\r\n\r\n"
        ));
        assert!(!is_allowed_connect(
            b"CONNECT chatgpt.com:443 HTTP/1.1\r\n\r\n"
        ));
        assert!(!is_allowed_connect(
            b"CONNECT ab.chatgpt.com:80 HTTP/1.1\r\n\r\n"
        ));
    }

    #[test]
    fn pac_url_is_a_local_http_endpoint() {
        assert_eq!(pac_url(19443), "http://127.0.0.1:19443/proxy.pac");
        assert!(is_pac_request(b"GET /proxy.pac HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n"));
    }
}
