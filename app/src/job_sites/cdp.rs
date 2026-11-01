//! Just enough of the browser's DevTools protocol (the one Edge and Chrome answer on
//! --remote-debugging-port) to open a page in a browser the app started, and read its HTML once
//! its JavaScript has shown it: a WebSocket of JSON commands, each answered by its id, with the
//! browser's events in between, which are let go.
//!
//! The browser listens on 127.0.0.1 only, on a port it picks, for as long as the app's read takes.

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

/// A connection to a browser's DevTools, and the page (its session) commands go to.
pub struct DevTools {
    socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
    /// The last command's id: each command gets the next one, which its answer carries.
    last_id: u64,
    /// The page's session, once a page is attached; commands without one go to the browser.
    session: Option<String>,
}

impl DevTools {
    /// Connects to the browser's DevTools at ws_url (ws://127.0.0.1:port/devtools/browser/…).
    pub async fn connect(ws_url: &str) -> Result<DevTools, String> {
        let (socket, _) = tokio_tungstenite::connect_async(ws_url)
            .await
            .map_err(|e| format!("its DevTools didn't answer: {e}"))?;
        Ok(DevTools {
            socket,
            last_id: 0,
            session: None,
        })
    }

    /// Opens a new blank page and sends the commands after this to it.
    pub async fn open_page(&mut self) -> Result<(), String> {
        let created = self
            .browser_call("Target.createTarget", json!({"url": "about:blank"}))
            .await?;
        let target = created["targetId"].as_str().ok_or("it opened no page")?.to_string();
        let attached = self
            .browser_call("Target.attachToTarget", json!({"targetId": target, "flatten": true}))
            .await?;
        let session = attached["sessionId"].as_str().ok_or("it didn't attach to its page")?;
        self.session = Some(session.to_string());
        Ok(())
    }

    /// Opens url in the page.
    pub async fn navigate(&mut self, url: &str) -> Result<(), String> {
        let answer = self.call("Page.navigate", json!({"url": url})).await?;
        match answer["errorText"].as_str() {
            Some(error) if !error.is_empty() => Err(format!("it couldn't open the page: {error}")),
            _ => Ok(()),
        }
    }

    /// Where the page is now, and its HTML as its JavaScript has made it so far.
    pub async fn location_and_html(&mut self) -> Result<(String, String), String> {
        /// The page's address on the first line, then its HTML.
        const EXPRESSION: &str = "location.href + '\\n' + document.documentElement.outerHTML";
        let answer = self
            .call(
                "Runtime.evaluate",
                json!({"expression": EXPRESSION, "returnByValue": true}),
            )
            .await?;
        let value = answer["result"]["value"].as_str().unwrap_or("");
        let (location, html) = value.split_once('\n').unwrap_or((value, ""));
        Ok((location.to_string(), html.to_string()))
    }

    /// Asks the browser to close (it ends with its last window); what it answers doesn't matter.
    pub async fn close_browser(&mut self) {
        let _ = self.browser_call("Browser.close", json!({})).await;
    }

    /// Sends method to the page (or to the browser before a page is attached), and its answer.
    async fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let session = self.session.clone();
        self.send(method, params, session).await
    }

    /// Sends method to the browser itself, and its answer.
    async fn browser_call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.send(method, params, None).await
    }

    async fn send(&mut self, method: &str, params: Value, session: Option<String>) -> Result<Value, String> {
        self.last_id += 1;
        let id = self.last_id;
        let mut command = json!({"id": id, "method": method, "params": params});
        if let Some(session) = session {
            command["sessionId"] = Value::String(session);
        }
        self.socket
            .send(Message::Text(command.to_string().into()))
            .await
            .map_err(|e| format!("its DevTools connection broke: {e}"))?;
        loop {
            let message = self
                .socket
                .next()
                .await
                .ok_or("its DevTools connection closed")?
                .map_err(|e| format!("its DevTools connection broke: {e}"))?;
            let Message::Text(text) = message else {
                continue;
            };
            let Ok(answer) = serde_json::from_str::<Value>(&text) else {
                continue;
            };
            if answer["id"].as_u64() != Some(id) {
                // An event, or the answer to a command given up on.
                continue;
            }
            if let Some(error) = answer.get("error") {
                let said = error["message"].as_str().unwrap_or("an error");
                return Err(format!("it answered {method} with {said}"));
            }
            return Ok(answer["result"].clone());
        }
    }
}

/// The DevTools address in a DevToolsActivePort file's text: its port on the first line, the
/// browser's path on the second.
pub fn address_of(active_port: &str) -> Option<String> {
    let mut lines = active_port.lines();
    let port: u16 = lines.next()?.trim().parse().ok()?;
    let path = lines.next()?.trim();
    path.starts_with('/').then(|| format!("ws://127.0.0.1:{port}{path}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_address_is_read_from_the_port_file() {
        assert_eq!(
            address_of("51234\n/devtools/browser/0b1c\n").as_deref(),
            Some("ws://127.0.0.1:51234/devtools/browser/0b1c")
        );
    }

    #[test]
    fn a_half_written_port_file_is_no_address() {
        assert_eq!(address_of("51234"), None);
        assert_eq!(address_of(""), None);
        assert_eq!(address_of("x\n/devtools/browser/0b1c"), None);
    }
}
