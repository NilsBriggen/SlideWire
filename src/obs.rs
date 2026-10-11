//! OBS Studio remote control over obs-websocket 5 (built into OBS 28+;
//! Tools → WebSocket Server Settings). One short connection per action.

use std::net::TcpStream;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tungstenite::{Message, WebSocket};

/// What a button does in OBS.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ObsOp {
    /// Switch the program scene.
    Scene {
        name: String,
    },
    /// Mute or unmute an audio source.
    ToggleMute {
        input: String,
    },
    ToggleRecord,
    ToggleStream,
}

impl ObsOp {
    fn request(&self) -> (&'static str, Value) {
        match self {
            ObsOp::Scene { name } => ("SetCurrentProgramScene", json!({ "sceneName": name })),
            ObsOp::ToggleMute { input } => ("ToggleInputMute", json!({ "inputName": input })),
            ObsOp::ToggleRecord => ("ToggleRecord", json!({})),
            ObsOp::ToggleStream => ("ToggleStream", json!({})),
        }
    }
}

/// Run `op` against OBS at `url` (e.g. ws://localhost:4455).
pub fn run(url: &str, password: &str, op: &ObsOp) -> Result<()> {
    let (kind, data) = op.request();
    call(url, password, kind, data).map(|_| ())
}

/// Connect and log in; returns OBS's version, to test the settings.
pub fn check(url: &str, password: &str) -> Result<String> {
    let v = call(url, password, "GetVersion", json!({}))?;
    Ok(v.pointer("/obsVersion").and_then(|v| v.as_str()).unwrap_or("?").to_string())
}

/// Names of OBS's scenes.
pub fn scenes(url: &str, password: &str) -> Result<Vec<String>> {
    let v = call(url, password, "GetSceneList", json!({}))?;
    let mut names: Vec<String> = v
        .get("scenes")
        .and_then(|s| s.as_array())
        .map(|a| a.iter().filter_map(|s| s.get("sceneName")?.as_str().map(String::from)).collect())
        .unwrap_or_default();
    // OBS lists scenes bottom to top.
    names.reverse();
    Ok(names)
}

fn call(url: &str, password: &str, kind: &str, data: Value) -> Result<Value> {
    let mut ws = connect(url)?;
    // Hello, possibly with an authentication challenge.
    let hello = recv(&mut ws)?;
    let mut identify = json!({ "rpcVersion": 1, "eventSubscriptions": 0 });
    if let Some(auth) = hello.pointer("/d/authentication") {
        if password.is_empty() {
            bail!("OBS asks for a password; set it in Settings");
        }
        let challenge = auth.get("challenge").and_then(|v| v.as_str()).unwrap_or_default();
        let salt = auth.get("salt").and_then(|v| v.as_str()).unwrap_or_default();
        identify["authentication"] = json!(auth_string(password, salt, challenge));
    }
    send(&mut ws, json!({ "op": 1, "d": identify }))?;
    let identified = recv(&mut ws).context("OBS closed the connection (wrong password?)")?;
    if identified.get("op").and_then(|v| v.as_u64()) != Some(2) {
        bail!("OBS didn't accept the login");
    }
    send(&mut ws, json!({ "op": 6, "d": { "requestType": kind, "requestId": "1", "requestData": data } }))?;
    loop {
        let msg = recv(&mut ws)?;
        if msg.get("op").and_then(|v| v.as_u64()) != Some(7) {
            continue;
        }
        let status = msg.pointer("/d/requestStatus").cloned().unwrap_or_default();
        let _ = ws.close(None);
        if status.get("result").and_then(|v| v.as_bool()) == Some(true) {
            return Ok(msg.pointer("/d/responseData").cloned().unwrap_or_default());
        }
        let comment = status.get("comment").and_then(|v| v.as_str()).unwrap_or("request failed");
        bail!("OBS: {comment}");
    }
}

fn connect(url: &str) -> Result<WebSocket<TcpStream>> {
    let host = url.trim_start_matches("ws://").split('/').next().unwrap_or_default();
    let addr = std::net::ToSocketAddrs::to_socket_addrs(host)
        .with_context(|| format!("bad OBS address {url}"))?
        .next()
        .ok_or_else(|| anyhow!("bad OBS address {url}"))?;
    let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2))
        .with_context(|| format!("OBS isn't reachable at {url}; is its WebSocket server on?"))?;
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    let (ws, _) = tungstenite::client(url, stream).map_err(|e| anyhow!("OBS handshake failed: {e}"))?;
    Ok(ws)
}

fn send(ws: &mut WebSocket<TcpStream>, v: Value) -> Result<()> {
    ws.send(Message::text(v.to_string()))?;
    Ok(())
}

fn recv(ws: &mut WebSocket<TcpStream>) -> Result<Value> {
    loop {
        match ws.read()? {
            Message::Text(t) => return Ok(serde_json::from_str(t.as_str())?),
            Message::Close(_) => bail!("OBS closed the connection"),
            _ => {}
        }
    }
}

/// base64(sha256(base64(sha256(password + salt)) + challenge)).
fn auth_string(password: &str, salt: &str, challenge: &str) -> String {
    let b64 = base64::engine::general_purpose::STANDARD;
    let secret = b64.encode(Sha256::digest(format!("{password}{salt}")));
    b64.encode(Sha256::digest(format!("{secret}{challenge}")))
}

#[cfg(test)]
mod tests {
    #[test]
    fn auth_matches_the_protocol_example() {
        // The example from the obs-websocket protocol documentation.
        assert_eq!(
            super::auth_string(
                "supersecretpassword",
                "lM1GncleQOaCu9lT1yeUZhFYnqhsLLP1G5lAGo3ixaI=",
                "+IxH4CnCiqpX1rM9scsNynZzbOe4KhDeYcTNS3PDaeY="
            ),
            "1Ct943GAT+6YQUUX47Ia/ncufilbe6+oD6lY+5kaCu4="
        );
    }
}
