use std::{
    io,
    net::SocketAddr,
    path::{Component, Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use axum::{
    Router,
    body::Body,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, header},
    response::{IntoResponse, Response},
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use interprocess::local_socket::{
    GenericFilePath,
    tokio::{Stream, prelude::*},
};
use percent_encoding::percent_decode_str;
use tokio::{
    fs::File,
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tokio_util::io::ReaderStream;
use zz_protocol::{MAX_ENCODED_FRAME_BYTES, MAX_FRAME_BYTES};

pub struct GatewayConfig {
    pub bind: SocketAddr,
    pub assets: PathBuf,
    pub socket: PathBuf,
}

pub struct Gateway {
    listener: TcpListener,
    address: SocketAddr,
    router: Router,
}

struct GatewayState {
    address: SocketAddr,
    assets: PathBuf,
    socket: PathBuf,
}

impl Gateway {
    pub async fn bind(config: GatewayConfig) -> io::Result<Self> {
        if !config.bind.ip().is_loopback() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the browser gateway must bind to a loopback address",
            ));
        }
        let assets = tokio::fs::canonicalize(config.assets).await?;
        if !tokio::fs::metadata(&assets).await?.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the asset path must be a directory",
            ));
        }
        let listener = TcpListener::bind(config.bind).await?;
        let address = listener.local_addr()?;
        let state = Arc::new(GatewayState {
            address,
            assets,
            socket: config.socket,
        });
        let router = Router::new()
            .route("/ws", get(upgrade))
            .fallback(asset)
            .with_state(state);
        Ok(Self {
            listener,
            address,
            router,
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.address
    }

    pub async fn serve(self) -> io::Result<()> {
        axum::serve(self.listener, self.router).await
    }
}

impl GatewayState {
    fn accepts_host(&self, headers: &HeaderMap) -> bool {
        let Some(host) = single_header(headers, header::HOST) else {
            return false;
        };
        host == self.address.to_string() || host == format!("localhost:{}", self.address.port())
    }

    fn accepts_origin(&self, headers: &HeaderMap) -> bool {
        self.accepts_host(headers)
            && single_header(headers, header::HOST)
                .zip(single_header(headers, header::ORIGIN))
                .is_some_and(|(host, origin)| origin == format!("http://{host}"))
    }
}

fn single_header(headers: &HeaderMap, name: header::HeaderName) -> Option<&str> {
    let mut values = headers.get_all(name).iter();
    let value = values.next()?.to_str().ok()?;
    values.next().is_none().then_some(value)
}

async fn upgrade(
    State(state): State<Arc<GatewayState>>,
    headers: HeaderMap,
    websocket: WebSocketUpgrade,
) -> Response {
    if !state.accepts_origin(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Ok(name) = state.socket.as_os_str().to_fs_name::<GenericFilePath>() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let Ok(Ok(daemon)) = tokio::time::timeout(Duration::from_secs(5), Stream::connect(name)).await
    else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    websocket
        .max_message_size(MAX_ENCODED_FRAME_BYTES)
        .max_frame_size(MAX_ENCODED_FRAME_BYTES)
        .max_write_buffer_size(MAX_ENCODED_FRAME_BYTES * 2)
        .on_upgrade(move |websocket| async move {
            let _ = bridge(websocket, daemon).await;
        })
}

async fn bridge(websocket: WebSocket, daemon: Stream) -> io::Result<()> {
    let (mut browser_write, mut browser_read) = websocket.split();
    let (mut daemon_read, mut daemon_write) = daemon.split();
    let browser_to_daemon = async {
        while let Some(message) = browser_read.next().await {
            match message.map_err(io::Error::other)? {
                Message::Binary(frame) => {
                    validate_frame(&frame)?;
                    daemon_write.write_all(&frame).await?;
                }
                Message::Close(_) => break,
                Message::Ping(_) | Message::Pong(_) => {}
                Message::Text(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "the zz connection requires binary protocol frames",
                    ));
                }
            }
        }
        Ok(())
    };
    let daemon_to_browser = async {
        loop {
            let frame = read_frame(&mut daemon_read).await?;
            browser_write
                .send(Message::Binary(frame.into()))
                .await
                .map_err(io::Error::other)?;
        }
    };
    tokio::select! {
        result = browser_to_daemon => result,
        result = daemon_to_browser => result,
    }
}

fn frame_length(prefix: [u8; 4]) -> io::Result<usize> {
    let length = u32::from_le_bytes(prefix) as usize;
    if !(4..=MAX_FRAME_BYTES).contains(&length) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid zz protocol frame length",
        ));
    }
    Ok(length)
}

fn validate_frame(frame: &[u8]) -> io::Result<()> {
    let prefix = frame
        .get(..4)
        .and_then(|prefix| prefix.try_into().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "truncated zz protocol frame"))?;
    if frame_length(prefix)? + 4 != frame.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "one WebSocket message must contain exactly one zz protocol frame",
        ));
    }
    Ok(())
}

async fn read_frame(reader: &mut (impl AsyncRead + Unpin)) -> io::Result<Vec<u8>> {
    let mut prefix = [0; 4];
    reader.read_exact(&mut prefix).await?;
    let length = frame_length(prefix)?;
    let mut frame = vec![0; length + 4];
    frame[..4].copy_from_slice(&prefix);
    reader.read_exact(&mut frame[4..]).await?;
    Ok(frame)
}

async fn asset(
    State(state): State<Arc<GatewayState>>,
    headers: HeaderMap,
    method: Method,
    uri: Uri,
) -> Response {
    if !state.accepts_host(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if method != Method::GET && method != Method::HEAD {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    let Some(relative) = asset_path(uri.path()) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Ok(path) = tokio::fs::canonicalize(state.assets.join(relative)).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !path.starts_with(&state.assets) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Ok(file) = File::open(&path).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Ok(metadata) = file.metadata().await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !metadata.is_file() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let body = if method == Method::HEAD {
        Body::empty()
    } else {
        Body::from_stream(ReaderStream::new(file))
    };
    let mut response = body.into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(content_type(&path)),
    );
    if let Ok(length) = HeaderValue::from_str(&metadata.len().to_string()) {
        headers.insert(header::CONTENT_LENGTH, length);
    }
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        "Cross-Origin-Opener-Policy",
        HeaderValue::from_static("same-origin"),
    );
    headers.insert(
        "Cross-Origin-Embedder-Policy",
        HeaderValue::from_static("require-corp"),
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("frame-ancestors 'none'"),
    );
    response
}

fn asset_path(path: &str) -> Option<PathBuf> {
    let path = percent_decode_str(path).decode_utf8().ok()?;
    if path.contains(['\\', '\0']) {
        return None;
    }
    let relative = path.strip_prefix('/')?;
    if relative.is_empty() {
        return Some(PathBuf::from("index.html"));
    }
    if relative
        .split('/')
        .any(|component| matches!(component, "." | ".." | ""))
    {
        return None;
    }
    let path = Path::new(relative);
    path.components()
        .all(|component| matches!(component, Component::Normal(_)))
        .then(|| path.to_path_buf())
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_frames_are_complete_and_bounded() {
        assert!(validate_frame(&[4, 0, 0, 0, 0, 0, 0, 0]).is_ok());
        assert!(validate_frame(&[4, 0, 0]).is_err());
        assert!(validate_frame(&[4, 0, 0, 0, 0, 0, 0]).is_err());
        assert!(validate_frame(&[4, 0, 0, 0, 0, 0, 0, 0, 0]).is_err());
        assert!(frame_length(3_u32.to_le_bytes()).is_err());
        assert!(frame_length((MAX_FRAME_BYTES as u32 + 1).to_le_bytes()).is_err());
    }

    #[tokio::test]
    async fn stream_frames_keep_boundaries_and_reject_truncation() {
        let first = [4, 0, 0, 0, 1, 2, 3, 4];
        let second = [5, 0, 0, 0, 5, 6, 7, 8, 9];
        let bytes = [first.as_slice(), second.as_slice()].concat();
        let mut reader = bytes.as_slice();
        assert_eq!(read_frame(&mut reader).await.unwrap(), first);
        assert_eq!(read_frame(&mut reader).await.unwrap(), second);
        assert!(read_frame(&mut reader).await.is_err());
        assert!(read_frame(&mut &first[..7]).await.is_err());
    }

    #[test]
    fn asset_paths_reject_encoded_and_platform_traversal() {
        assert_eq!(asset_path("/"), Some(PathBuf::from("index.html")));
        assert_eq!(
            asset_path("/pkg/client.wasm"),
            Some(PathBuf::from("pkg/client.wasm"))
        );
        for path in [
            "/../secret",
            "/%2e%2e/secret",
            "/pkg/%2f../secret",
            "/pkg/./client.js",
            "/pkg\\secret",
            "/%5csecret",
            "/%00secret",
            "//secret",
            "/pkg//secret",
        ] {
            assert!(asset_path(path).is_none(), "accepted {path}");
        }
    }
}
