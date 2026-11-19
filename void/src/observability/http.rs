//! A deliberately small HTTP/1.1 responder: one `GET` or `HEAD` request per
//! connection, answered and closed. Enough for health checks and Prometheus
//! scrapes, nothing more.

use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Requests whose head (request line and headers) is larger are answered
/// with `431`.
pub(crate) const MAX_REQUEST_HEAD_BYTES: usize = 8 * 1024;
/// Time a client has to send its request head, and then to read the response.
pub(crate) const REQUEST_DEADLINE: Duration = Duration::from_secs(5);
/// After answering, unread request bytes are discarded for at most this long
/// so that closing the socket does not reset it before the client has read the
/// response.
const LINGER: Duration = Duration::from_secs(1);
const MAX_LINGER_BYTES: usize = 64 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub body: String,
    pub allow: Option<&'static str>,
}

impl Response {
    pub(crate) fn new(status: u16, content_type: &'static str, body: String) -> Self {
        Self {
            status,
            content_type,
            body,
            allow: None,
        }
    }

    pub(crate) fn text(status: u16, body: &str) -> Self {
        Self::new(status, "text/plain; charset=utf-8", format!("{body}\n"))
    }

    fn encode(&self, include_body: bool) -> Vec<u8> {
        let mut head = format!(
            "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n",
            self.status,
            reason(self.status),
            self.content_type,
            self.body.len()
        );
        if let Some(allow) = self.allow {
            head.push_str("Allow: ");
            head.push_str(allow);
            head.push_str("\r\n");
        }
        head.push_str("\r\n");
        let mut bytes = head.into_bytes();
        if include_body {
            bytes.extend_from_slice(self.body.as_bytes());
        }
        bytes
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        431 => "Request Header Fields Too Large",
        503 => "Service Unavailable",
        505 => "HTTP Version Not Supported",
        _ => "Unknown",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Method {
    Get,
    Head,
    Other,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Request<'a> {
    pub method: Method,
    /// The request target without its query string.
    pub path: &'a str,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ParseError {
    Malformed,
    UnsupportedVersion,
}

/// Parses the request line of a request head. Headers are not needed by any
/// route and are ignored.
pub(crate) fn parse_request(head: &[u8]) -> Result<Request<'_>, ParseError> {
    let line_end = head
        .windows(2)
        .position(|pair| pair == b"\r\n")
        .unwrap_or(head.len());
    let line = std::str::from_utf8(&head[..line_end]).map_err(|_| ParseError::Malformed)?;
    let mut parts = line.split(' ');
    let (Some(method), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(ParseError::Malformed);
    };
    if method.is_empty() || !method.bytes().all(is_token_byte) {
        return Err(ParseError::Malformed);
    }
    if !target.starts_with('/') || !target.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(ParseError::Malformed);
    }
    match version {
        "HTTP/1.1" | "HTTP/1.0" => {}
        _ if version.starts_with("HTTP/") => return Err(ParseError::UnsupportedVersion),
        _ => return Err(ParseError::Malformed),
    }
    let method = match method {
        "GET" => Method::Get,
        "HEAD" => Method::Head,
        _ => Method::Other,
    };
    let path = target.split_once('?').map_or(target, |(path, _)| path);
    Ok(Request { method, path })
}

fn is_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
}

#[derive(Debug)]
enum ReadError {
    TooLarge,
    Closed,
}

/// Reads until the blank line that ends a request head and returns the head's
/// length, without the blank line.
async fn read_head<R: AsyncRead + Unpin>(
    reader: &mut R,
    buffer: &mut Vec<u8>,
) -> Result<usize, ReadError> {
    let mut chunk = [0u8; 1024];
    loop {
        if let Some(end) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            return Ok(end);
        }
        if buffer.len() >= MAX_REQUEST_HEAD_BYTES {
            return Err(ReadError::TooLarge);
        }
        let room = (MAX_REQUEST_HEAD_BYTES - buffer.len()).min(chunk.len());
        match reader.read(&mut chunk[..room]).await {
            Ok(0) | Err(_) => return Err(ReadError::Closed),
            Ok(read) => buffer.extend_from_slice(&chunk[..read]),
        }
    }
}

/// Serves one request on `stream` with `route`, then closes it. Nothing here
/// waits longer than `deadline` (plus a short linger) for the client.
pub(crate) async fn serve_connection<S, F>(mut stream: S, deadline: Duration, route: F)
where
    S: AsyncRead + AsyncWrite + Unpin,
    F: FnOnce(&Request<'_>) -> Response,
{
    let mut buffer = Vec::new();
    let (response, include_body) =
        match tokio::time::timeout(deadline, read_head(&mut stream, &mut buffer)).await {
            Err(_) => (Response::text(408, "request timeout"), true),
            Ok(Err(ReadError::Closed)) => return,
            Ok(Err(ReadError::TooLarge)) => (Response::text(431, "request head too large"), true),
            Ok(Ok(head_len)) => match parse_request(&buffer[..head_len]) {
                Ok(request) => (route(&request), request.method != Method::Head),
                Err(ParseError::Malformed) => (Response::text(400, "malformed request"), true),
                Err(ParseError::UnsupportedVersion) => {
                    (Response::text(505, "only HTTP/1.0 and HTTP/1.1"), true)
                }
            },
        };

    let bytes = response.encode(include_body);
    let written = tokio::time::timeout(deadline, async {
        stream.write_all(&bytes).await?;
        stream.shutdown().await
    })
    .await;
    if !matches!(written, Ok(Ok(()))) {
        return;
    }
    let _ = tokio::time::timeout(LINGER, async {
        let mut sink = [0u8; 1024];
        let mut discarded = 0;
        while discarded < MAX_LINGER_BYTES {
            match stream.read(&mut sink).await {
                Ok(0) | Err(_) => break,
                Ok(read) => discarded += read,
            }
        }
    })
    .await;
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt, duplex};

    use super::*;

    #[test]
    fn parses_get_and_head_and_strips_the_query() {
        assert_eq!(
            parse_request(b"GET /metrics HTTP/1.1\r\nHost: x"),
            Ok(Request {
                method: Method::Get,
                path: "/metrics"
            })
        );
        assert_eq!(
            parse_request(b"HEAD /health?verbose=1 HTTP/1.0"),
            Ok(Request {
                method: Method::Head,
                path: "/health"
            })
        );
        assert_eq!(
            parse_request(b"POST /health HTTP/1.1").map(|r| r.method),
            Ok(Method::Other)
        );
    }

    #[test]
    fn rejects_malformed_request_lines() {
        for head in [
            &b""[..],
            b"GET",
            b"GET /health",
            b"GET  /health HTTP/1.1",
            b"GET /health HTTP/1.1 extra",
            b"GET health HTTP/1.1",
            b"G(T /health HTTP/1.1",
            b"GET /he\xffalth HTTP/1.1",
            b"GET /health FTP/1.0",
        ] {
            assert_eq!(
                parse_request(head),
                Err(ParseError::Malformed),
                "{:?}",
                String::from_utf8_lossy(head)
            );
        }
        assert_eq!(
            parse_request(b"GET /health HTTP/2.0"),
            Err(ParseError::UnsupportedVersion)
        );
    }

    async fn exchange(request: &[u8], deadline: Duration) -> String {
        let (mut client, server) = duplex(64 * 1024);
        let serving = tokio::spawn(serve_connection(server, deadline, |request| {
            Response::text(200, request.path)
        }));
        client.write_all(request).await.unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).await.unwrap();
        drop(client);
        serving.await.unwrap();
        response
    }

    #[tokio::test]
    async fn answers_one_request_and_closes() {
        let response = exchange(b"GET /a?b HTTP/1.1\r\nHost: x\r\n\r\n", REQUEST_DEADLINE).await;
        assert_eq!(
            response,
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: 3\r\n\
             Cache-Control: no-store\r\nConnection: close\r\n\r\n/a\n"
        );
    }

    #[tokio::test]
    async fn head_requests_get_headers_only() {
        let response = exchange(b"HEAD /a HTTP/1.1\r\n\r\n", REQUEST_DEADLINE).await;
        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(response.contains("Content-Length: 3\r\n"));
        assert!(response.ends_with("\r\n\r\n"));
    }

    #[tokio::test]
    async fn malformed_requests_get_400() {
        let response = exchange(b"\x16\x03\x01garbage\r\n\r\n", REQUEST_DEADLINE).await;
        assert!(
            response.starts_with("HTTP/1.1 400 Bad Request\r\n"),
            "{response}"
        );
    }

    #[tokio::test]
    async fn oversized_request_heads_get_431() {
        let mut request = b"GET /a HTTP/1.1\r\nX-Padding: ".to_vec();
        request.resize(MAX_REQUEST_HEAD_BYTES * 2, b'a');
        let response = exchange(&request, REQUEST_DEADLINE).await;
        assert!(
            response.starts_with("HTTP/1.1 431 Request Header Fields Too Large\r\n"),
            "{response}"
        );
    }

    #[tokio::test]
    async fn clients_that_never_finish_their_request_get_408() {
        let response = exchange(b"GET /a HTTP/1.1\r\n", Duration::from_millis(50)).await;
        assert!(
            response.starts_with("HTTP/1.1 408 Request Timeout\r\n"),
            "{response}"
        );
    }

    #[tokio::test]
    async fn clients_that_close_early_get_nothing() {
        let (client, server) = duplex(1024);
        drop(client);
        serve_connection(server, REQUEST_DEADLINE, |_| unreachable!()).await;
    }
}
