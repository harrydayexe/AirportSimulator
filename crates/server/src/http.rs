//! Just enough HTTP/1.1 for the client port: one request per connection,
//! JSON out, a thread per connection.

use std::io::{self, BufRead as _, BufReader, Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

/// The most request headers read, in bytes.
const MAX_HEAD_BYTES: u64 = 16 * 1024;
/// The largest request body accepted, in bytes.
const MAX_BODY_BYTES: usize = 1024 * 1024;
/// How long a client may take to send its request or read the answer.
const IO_TIMEOUT: Duration = Duration::from_secs(10);
/// Connections served at once before new ones are turned away.
const MAX_CONNECTIONS: usize = 256;

/// A request, as far as this server cares.
#[derive(Debug, PartialEq, Eq)]
pub struct Request {
    /// `GET`, `POST`, ...
    pub method: String,
    /// The path, without any query string.
    pub path: String,
    /// The body; empty when there is none.
    pub body: Vec<u8>,
}

/// A response: a status, a JSON body, and perhaps somewhere better to ask.
#[derive(Debug, PartialEq, Eq)]
pub struct Response {
    /// The status code.
    pub status: u16,
    /// The JSON body.
    pub body: String,
    /// A `Location` header, for redirects.
    pub location: Option<String>,
}

impl Response {
    /// A response with `body` as its JSON.
    #[must_use]
    pub fn json(status: u16, body: &serde_json::Value) -> Self {
        Self {
            status,
            body: body.to_string(),
            location: None,
        }
    }

    /// `{"error": message}` with `status`.
    #[must_use]
    pub fn error(status: u16, message: &str) -> Self {
        Self::json(status, &serde_json::json!({ "error": message }))
    }
}

/// The words that go with a status code.
fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        307 => "Temporary Redirect",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Payload Too Large",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Error",
    }
}

/// Reads one request. `Err` carries the response to send instead.
fn read_request(stream: &TcpStream) -> Result<Request, Response> {
    let bad = |what: &str| Response::error(400, what);
    let mut reader = BufReader::new(stream.take(MAX_HEAD_BYTES));
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|_| bad("unreadable request"))?;
    let mut parts = line.split_whitespace();
    let (Some(method), Some(target), Some(_version)) = (parts.next(), parts.next(), parts.next())
    else {
        return Err(bad("not an HTTP request"));
    };
    let path = target.split('?').next().unwrap_or(target).to_owned();
    let method = method.to_owned();

    let mut length = 0;
    loop {
        line.clear();
        let read = reader
            .read_line(&mut line)
            .map_err(|_| bad("unreadable headers"))?;
        if read == 0 {
            return Err(bad("headers too long or cut short"));
        }
        let header = line.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            length = value
                .trim()
                .parse()
                .map_err(|_| bad("bad Content-Length"))?;
        }
    }
    if length > MAX_BODY_BYTES {
        return Err(Response::error(413, "request body too large"));
    }
    // The body may be partly in the reader's buffer; lift the header limit.
    reader.get_mut().set_limit(length as u64);
    let mut body = vec![0; length];
    reader
        .read_exact(&mut body)
        .map_err(|_| bad("body cut short"))?;
    Ok(Request { method, path, body })
}

/// Writes `response` and closes the connection.
fn write_response(mut stream: &TcpStream, response: &Response) -> io::Result<()> {
    let location = response
        .location
        .as_ref()
        .map_or_else(String::new, |to| format!("Location: {to}\r\n"));
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{location}Connection: close\r\n\r\n",
        response.status,
        reason(response.status),
        response.body.len(),
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(response.body.as_bytes())?;
    stream.flush()
}

/// Serves `listener` for ever, answering each request with `handler`. Only
/// returns if the listener fails.
///
/// # Errors
/// If a thread cannot be started.
pub fn serve<H>(listener: TcpListener, handler: H) -> io::Result<()>
where
    H: Fn(Request) -> Response + Send + Sync + 'static,
{
    let handler = Arc::new(handler);
    let open = Arc::new(AtomicUsize::new(0));
    thread::Builder::new()
        .name("accept-clients".to_owned())
        .spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
                let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
                if open.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                    open.fetch_sub(1, Ordering::SeqCst);
                    let _ = write_response(&stream, &Response::error(503, "too many connections"));
                    continue;
                }
                let (handler, open) = (Arc::clone(&handler), Arc::clone(&open));
                let serve_one = move || {
                    let response =
                        read_request(&stream).map_or_else(|refusal| refusal, |r| handler(r));
                    let _ = write_response(&stream, &response);
                    open.fetch_sub(1, Ordering::SeqCst);
                };
                if thread::Builder::new()
                    .name("client".to_owned())
                    .spawn(serve_one)
                    .is_err()
                {
                    // The closure, and the connection with it, is dropped.
                    eprintln!("cannot start a thread for a client connection");
                }
            }
        })?;
    Ok(())
}

/// Sends one request to `address` and returns the status and body. Used by
/// `airport-server healthcheck` and by tests.
///
/// # Errors
/// If the server cannot be reached or does not answer with HTTP.
pub fn request(address: &str, method: &str, path: &str, body: &str) -> io::Result<(u16, String)> {
    let invalid = |what: &str| io::Error::new(io::ErrorKind::InvalidData, what.to_owned());
    let mut stream = TcpStream::connect(address)?;
    stream.set_read_timeout(Some(IO_TIMEOUT * 2))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    let mut answer = String::new();
    stream.read_to_string(&mut answer)?;
    let (head, body) = answer
        .split_once("\r\n\r\n")
        .ok_or_else(|| invalid("no header end"))?;
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| invalid("no status"))?;
    Ok((status, body.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        serve(listener, |request| {
            let body = String::from_utf8_lossy(&request.body).into_owned();
            let mut response = Response::json(
                if request.path == "/moved" { 307 } else { 200 },
                &serde_json::json!({ "method": request.method, "path": request.path, "body": body }),
            );
            response.location = (request.path == "/moved").then(|| "http://elsewhere/".to_owned());
            response
        })
        .unwrap();
        address
    }

    #[test]
    fn a_request_gets_its_answer() {
        let address = server();
        let (status, body) = request(&address, "POST", "/commands?x=1", "{\"a\":1}").unwrap();
        assert_eq!(status, 200);
        let body: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(body["method"], "POST");
        assert_eq!(body["path"], "/commands", "the query string is dropped");
        assert_eq!(body["body"], "{\"a\":1}");
        assert_eq!(request(&address, "GET", "/", "").unwrap().0, 200);
    }

    #[test]
    fn a_redirect_names_where_to_go() {
        let address = server();
        let mut stream = TcpStream::connect(&address).unwrap();
        stream.write_all(b"GET /moved HTTP/1.1\r\n\r\n").unwrap();
        let mut answer = String::new();
        stream.read_to_string(&mut answer).unwrap();
        assert!(
            answer.starts_with("HTTP/1.1 307 Temporary Redirect\r\n"),
            "{answer}"
        );
        assert!(
            answer.contains("\r\nLocation: http://elsewhere/\r\n"),
            "{answer}"
        );
    }

    #[test]
    fn rubbish_and_oversized_requests_are_refused() {
        let address = server();
        let raw = |text: &str| {
            let mut stream = TcpStream::connect(&address).unwrap();
            stream.write_all(text.as_bytes()).unwrap();
            let mut answer = String::new();
            let _ = stream.read_to_string(&mut answer);
            answer
        };
        assert!(raw("hello\r\n\r\n").starts_with("HTTP/1.1 400 "));
        assert!(raw("POST / HTTP/1.1\r\nContent-Length: x\r\n\r\n").starts_with("HTTP/1.1 400 "));
        assert!(
            raw("POST / HTTP/1.1\r\nContent-Length: 99999999\r\n\r\n").starts_with("HTTP/1.1 413 ")
        );
        let long = format!("GET / HTTP/1.1\r\nX: {}\r\n\r\n", "a".repeat(20_000));
        assert!(raw(&long).starts_with("HTTP/1.1 400 "));
    }
}
