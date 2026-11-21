//! The loopback listener the browser returns to (RFC 8252 §7.3): `http://127.0.0.1:<port>/callback`.
//!
//! Only `GET /callback` with the expected `state` completes the sign-in. Anything else gets an error
//! page and the listener keeps waiting, so a stray or forged request cannot end or hijack it.

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use url::Url;

use crate::error::AuthError;

pub const CALLBACK_PATH: &str = "/callback";
const MAX_REQUEST_HEAD: usize = 8 * 1024;
const READ_TIMEOUT: Duration = Duration::from_secs(10);

pub struct Loopback {
    listener: TcpListener,
    redirect_uri: Url,
}

impl Loopback {
    /// Binds an ephemeral port on `127.0.0.1` (never `localhost`, which may resolve elsewhere).
    pub async fn bind() -> Result<Self, AuthError> {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(|error| AuthError::LoopbackUnavailable(error.to_string()))?;
        let port = listener
            .local_addr()
            .map_err(|error| AuthError::LoopbackUnavailable(error.to_string()))?
            .port();
        let redirect_uri = Url::parse(&format!("http://127.0.0.1:{port}{CALLBACK_PATH}"))
            .expect("loopback URL is valid");
        Ok(Self {
            listener,
            redirect_uri,
        })
    }

    pub fn redirect_uri(&self) -> &Url {
        &self.redirect_uri
    }

    /// Waits for the browser, up to `timeout`, and returns the authorization code.
    pub async fn wait_for_code(
        self,
        expected_state: &str,
        timeout: Duration,
    ) -> Result<String, AuthError> {
        tokio::time::timeout(timeout, self.accept_until_callback(expected_state))
            .await
            .map_err(|_| AuthError::LoginTimedOut)?
    }

    async fn accept_until_callback(&self, expected_state: &str) -> Result<String, AuthError> {
        loop {
            let Ok((mut stream, _)) = self.listener.accept().await else {
                continue;
            };
            let Ok(Ok(target)) =
                tokio::time::timeout(READ_TIMEOUT, read_request_target(&mut stream)).await
            else {
                let _ = respond(&mut stream, 400, page::BAD_REQUEST).await;
                continue;
            };
            match classify(&target, expected_state) {
                Callback::Code(code) => {
                    let _ = respond(&mut stream, 200, page::SUCCESS).await;
                    return Ok(code);
                }
                Callback::Denied { error, description } => {
                    let _ = respond(&mut stream, 200, page::DENIED).await;
                    return Err(AuthError::Denied { error, description });
                }
                Callback::WrongState => {
                    tracing::warn!("sign-in callback with an unexpected state ignored");
                    let _ = respond(&mut stream, 400, page::BAD_REQUEST).await;
                }
                Callback::Other => {
                    let _ = respond(&mut stream, 404, page::NOT_FOUND).await;
                }
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Callback {
    Code(String),
    Denied {
        error: String,
        description: Option<String>,
    },
    WrongState,
    Other,
}

/// Decides what a request target (`/callback?code=…&state=…`) means.
pub(crate) fn classify(target: &str, expected_state: &str) -> Callback {
    let Ok(url) = Url::parse("http://127.0.0.1").and_then(|base| base.join(target)) else {
        return Callback::Other;
    };
    if url.path() != CALLBACK_PATH {
        return Callback::Other;
    }
    let mut code = None;
    let mut state = None;
    let mut error = None;
    let mut description = None;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "code" => code = Some(value.into_owned()),
            "state" => state = Some(value.into_owned()),
            "error" => error = Some(value.into_owned()),
            "error_description" => description = Some(value.into_owned()),
            _ => {}
        }
    }
    if state.as_deref() != Some(expected_state) {
        return Callback::WrongState;
    }
    match (code, error) {
        (_, Some(error)) => Callback::Denied { error, description },
        (Some(code), None) if !code.is_empty() => Callback::Code(code),
        _ => Callback::Other,
    }
}

/// Reads the request head and returns the target of a `GET` (`/callback?…`).
async fn read_request_target(stream: &mut TcpStream) -> std::io::Result<String> {
    let mut head = Vec::with_capacity(1024);
    let mut buf = [0u8; 1024];
    while !head.windows(4).any(|window| window == b"\r\n\r\n") {
        let read = stream.read(&mut buf).await?;
        if read == 0 || head.len() + read > MAX_REQUEST_HEAD {
            break;
        }
        head.extend_from_slice(&buf[..read]);
    }
    let text = String::from_utf8_lossy(&head);
    let mut first = text.lines().next().unwrap_or_default().split_whitespace();
    match (first.next(), first.next()) {
        (Some("GET"), Some(target)) => Ok(target.to_owned()),
        _ => Err(std::io::Error::other("not a GET")),
    }
}

async fn respond(stream: &mut TcpStream, status: u16, body: &str) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        _ => "Not Found",
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).await?;
    stream.shutdown().await
}

/// Self-contained pages (no external resources) shown in the browser tab.
mod page {
    macro_rules! page {
        ($title:literal, $text:literal) => {
            concat!(
                "<!doctype html><html lang=\"pt-BR\"><head><meta charset=\"utf-8\"><title>Medvita</title>",
                "<style>body{font-family:system-ui,sans-serif;display:flex;min-height:100vh;margin:0;",
                "align-items:center;justify-content:center;background:#f6f7f9;color:#1d2430}",
                "main{max-width:28rem;padding:2rem;text-align:center}h1{font-size:1.4rem}",
                "@media(prefers-color-scheme:dark){body{background:#14181f;color:#e6e9ef}}</style>",
                "</head><body><main><h1>", $title, "</h1><p>", $text, "</p></main></body></html>"
            )
        };
    }

    pub const SUCCESS: &str = page!(
        "Login concluído",
        "Você já pode fechar esta aba e voltar ao Medvita."
    );
    pub const DENIED: &str = page!(
        "Login não concluído",
        "O acesso não foi autorizado. Volte ao Medvita para tentar de novo."
    );
    pub const BAD_REQUEST: &str = page!(
        "Pedido inválido",
        "Esta página não corresponde a um login em andamento no Medvita."
    );
    pub const NOT_FOUND: &str = page!("Página não encontrada", "Volte ao Medvita.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_reads_code_error_and_state() {
        assert_eq!(
            classify("/callback?code=abc&state=s1", "s1"),
            Callback::Code("abc".into())
        );
        assert_eq!(
            classify(
                "/callback?error=access_denied&error_description=n%C3%A3o&state=s1",
                "s1"
            ),
            Callback::Denied {
                error: "access_denied".into(),
                description: Some("não".into())
            }
        );
        assert_eq!(
            classify("/callback?code=abc&state=other", "s1"),
            Callback::WrongState
        );
        assert_eq!(classify("/callback?code=abc", "s1"), Callback::WrongState);
        assert_eq!(classify("/favicon.ico", "s1"), Callback::Other);
        assert_eq!(classify("/callback?state=s1", "s1"), Callback::Other);
    }
}
