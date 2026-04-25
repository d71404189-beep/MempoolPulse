//! Mempool subscription worker.
//!
//! Connects to a user-supplied JSON-RPC WebSocket (typically Alchemy / QuickNode
//! / Infura on Ethereum mainnet) and streams pending transactions to the
//! frontend as Tauri events. We try the richer `alchemy_pendingTransactions`
//! subscription first (returns full tx bodies); on providers that don't support
//! it, we fall back to `eth_subscribe newPendingTransactions` (hashes only) and
//! enrich each tx with a follow-up `eth_getTransactionByHash` over HTTP.

use crate::decoder;
use crate::state::AppState;
use crate::types::{ConnectionStatus, Filters, PendingTx};
use anyhow::{anyhow, Context};
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::Message;

const EVENT_PENDING: &str = "mempool://pending";
const EVENT_STATUS: &str = "mempool://status";

/// Spawn the worker task. Cancels any previously running worker first.
pub async fn restart(app: AppHandle, state: AppState) {
    {
        let mut guard = state.worker.lock().await;
        if let Some(handle) = guard.take() {
            *state.shutdown.write() = true;
            handle.abort();
        }
        *state.shutdown.write() = false;
    }

    let app_clone = app.clone();
    let state_clone = state.clone();
    let handle = tokio::spawn(async move {
        run_with_reconnect(app_clone, state_clone).await;
    });
    *state.worker.lock().await = Some(handle);
}

async fn run_with_reconnect(app: AppHandle, state: AppState) {
    let mut backoff = Duration::from_secs(2);
    loop {
        if *state.shutdown.read() {
            return;
        }

        let url = { state.settings.read().rpc_ws_url.clone() };
        if url.is_empty() {
            update_status_keyed(&app, &state, false, "status.no_rpc", HashMap::new());
            tokio::time::sleep(Duration::from_secs(3)).await;
            continue;
        }

        let mut p = HashMap::new();
        p.insert("url".into(), redact(&url));
        update_status_keyed(&app, &state, false, "status.connecting", p);

        match run_once(&app, &state, &url).await {
            Ok(()) => {
                // Stream ended cleanly (rare); reconnect with short delay.
                update_status_keyed(&app, &state, false, "status.closed", HashMap::new());
                tokio::time::sleep(Duration::from_secs(2)).await;
                backoff = Duration::from_secs(2);
            }
            Err(e) => {
                let mut p = HashMap::new();
                p.insert("err".into(), e.to_string());
                p.insert("secs".into(), backoff.as_secs().to_string());
                update_status_keyed(&app, &state, false, "status.disconnected", p);
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(30));
            }
        }
    }
}

/// How long to wait for the first pending-tx notification after an
/// `alchemy_pendingTransactions` subscription is acknowledged. Some providers
/// (e.g. publicnode) silently accept the subscribe call and return an id but
/// never push notifications — if no notification arrives within this window
/// we unsubscribe and fall back to the standard `newPendingTransactions`.
const ALCHEMY_PROBE_SECS: u64 = 7;
const SUBSCRIBE_ACK_SECS: u64 = 5;

async fn run_once(app: &AppHandle, state: &AppState, ws_url: &str) -> anyhow::Result<()> {
    let request = ws_url
        .into_client_request()
        .with_context(|| format!("Invalid WebSocket URL: {}", ws_url))?;
    let (mut ws, _) = tokio_tungstenite::connect_async(request)
        .await
        .with_context(|| "WebSocket handshake failed")?;

    // Try the richer Alchemy subscription first.
    let alchemy_sub = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "eth_subscribe",
        "params": ["alchemy_pendingTransactions", { "hashesOnly": false }]
    });
    ws.send(Message::Text(alchemy_sub.to_string().into())).await?;

    let mut alchemy_sub_id: Option<String> = None;
    // Wait for the first response — either ack or error — with a timeout so
    // a provider that never replies doesn't wedge the worker forever.
    if let Ok(Some(Ok(Message::Text(text)))) =
        tokio::time::timeout(Duration::from_secs(SUBSCRIBE_ACK_SECS), ws.next()).await
    {
        let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        if let Some(id) = v.get("result").and_then(|r| r.as_str()) {
            alchemy_sub_id = Some(id.to_string());
        }
    }

    // If alchemy was acked, probe for an actual notification. Some providers
    // ack the subscribe call but never push — the explicit fallback below
    // covers that case. Any notification we receive during the probe is
    // buffered and replayed into the main loop so we don't drop it.
    let mut buffered: Option<Message> = None;
    if let Some(sub_id) = &alchemy_sub_id {
        match tokio::time::timeout(
            Duration::from_secs(ALCHEMY_PROBE_SECS),
            wait_for_pending_notification(&mut ws),
        )
        .await
        {
            Ok(Ok(Some(msg))) => {
                buffered = Some(msg);
            }
            // Timeout, ws error, or ws closed before any notification arrived.
            // Unsubscribe alchemy and fall back to the standard subscription.
            _ => {
                let unsub = json!({
                    "jsonrpc": "2.0",
                    "id": 99,
                    "method": "eth_unsubscribe",
                    "params": [sub_id]
                });
                ws.send(Message::Text(unsub.to_string().into())).await.ok();
                alchemy_sub_id = None;
            }
        }
    }

    if alchemy_sub_id.is_none() {
        // Fallback: subscribe to plain newPendingTransactions (hashes only).
        let std_sub = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "eth_subscribe",
            "params": ["newPendingTransactions"]
        });
        ws.send(Message::Text(std_sub.to_string().into())).await?;
        match tokio::time::timeout(Duration::from_secs(SUBSCRIBE_ACK_SECS), ws.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
                if v.get("error").is_some() || v.get("result").is_none() {
                    return Err(anyhow!(
                        "Both alchemy_pendingTransactions and newPendingTransactions subscriptions were rejected"
                    ));
                }
            }
            Ok(Some(Ok(Message::Close(_)))) | Ok(None) => {
                return Err(anyhow!("WebSocket closed before subscription was acknowledged"));
            }
            Ok(Some(Err(e))) => return Err(anyhow!(e)),
            Err(_) => {
                return Err(anyhow!(
                    "Timed out waiting for newPendingTransactions subscription ack"
                ));
            }
            _ => {}
        }
    }

    update_status_keyed(app, state, true, "status.streaming", HashMap::new());

    let http_url = { state.settings.read().rpc_http_url.clone() };
    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .ok();

    loop {
        let next = match buffered.take() {
            Some(m) => Some(Ok(m)),
            None => ws.next().await,
        };
        let Some(msg) = next else { break };
        if *state.shutdown.read() {
            return Ok(());
        }
        let msg = match msg {
            Ok(m) => m,
            Err(e) => return Err(anyhow!(e)),
        };
        let text = match msg {
            Message::Text(t) => t.to_string(),
            Message::Ping(p) => {
                ws.send(Message::Pong(p)).await.ok();
                continue;
            }
            Message::Close(_) => return Ok(()),
            _ => continue,
        };

        let value: Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let params = match value.get("params").and_then(|p| p.get("result")) {
            Some(p) => p.clone(),
            None => continue,
        };

        // Either a full tx object (alchemy) or a bare hash string (standard).
        let pending_tx = if params.is_object() {
            build_pending_tx(&params, state).await
        } else if let Some(hash) = params.as_str() {
            if let (Some(client), Some(http_url)) = (http_client.as_ref(), nonempty(&http_url)) {
                if let Some(tx) = fetch_tx_by_hash(client, http_url, hash).await {
                    build_pending_tx(&tx, state).await
                } else {
                    continue;
                }
            } else {
                // We only have a hash and no HTTP RPC to enrich it. Emit a stub.
                Some(stub_from_hash(hash))
            }
        } else {
            None
        };

        let Some(tx) = pending_tx else { continue };
        let filters = { state.settings.read().filters.clone() };
        if !passes_filters(&tx, &filters, state) {
            continue;
        }
        let _ = app.emit(EVENT_PENDING, &tx);
    }
    Ok(())
}

async fn fetch_tx_by_hash(
    client: &reqwest::Client,
    http_url: &str,
    hash: &str,
) -> Option<Value> {
    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "eth_getTransactionByHash",
        "params": [hash]
    });
    let resp = client.post(http_url).json(&body).send().await.ok()?;
    let json: Value = resp.json().await.ok()?;
    json.get("result").cloned().filter(|r| !r.is_null())
}

async fn build_pending_tx(raw: &Value, state: &AppState) -> Option<PendingTx> {
    let hash = raw.get("hash")?.as_str()?.to_string();
    let from = raw
        .get("from")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_lowercase();
    let to = raw
        .get("to")
        .and_then(|v| v.as_str())
        .map(|s| s.to_lowercase());
    let value_hex = raw.get("value").and_then(|v| v.as_str()).unwrap_or("0x0");
    let value_wei = decoder::hex_to_decimal_string(value_hex);
    let value_wei_f = decoder::parse_hex_wei(value_hex).unwrap_or(0.0);
    let value_eth = value_wei_f / 1e18;

    let gas_gwei = raw
        .get("maxFeePerGas")
        .and_then(|v| v.as_str())
        .or_else(|| raw.get("gasPrice").and_then(|v| v.as_str()))
        .and_then(decoder::parse_hex_wei)
        .map(|wei| wei / 1e9);
    let gas_limit = raw
        .get("gas")
        .and_then(|v| v.as_str())
        .map(decoder::hex_to_decimal_string);

    let input = raw
        .get("input")
        .and_then(|v| v.as_str())
        .unwrap_or("0x")
        .to_string();
    let selector = if input.len() >= 10 {
        Some(input[..10].to_lowercase())
    } else {
        None
    };
    let (label, summary) = decoder::decode(&input);

    let eth_usd = state.prices.eth_usd().await;
    let value_usd = if eth_usd > 0.0 {
        Some(value_eth * eth_usd)
    } else {
        None
    };

    Some(PendingTx {
        hash,
        from,
        to,
        value_wei,
        value_eth,
        value_usd,
        gas_gwei,
        gas_limit,
        input,
        selector,
        label,
        summary,
        seen_at: Utc::now().timestamp(),
    })
}

fn stub_from_hash(hash: &str) -> PendingTx {
    PendingTx {
        hash: hash.to_string(),
        from: String::new(),
        to: None,
        value_wei: "0".into(),
        value_eth: 0.0,
        value_usd: None,
        gas_gwei: None,
        gas_limit: None,
        input: "0x".into(),
        selector: None,
        label: Some("Pending (hash only)".into()),
        summary: Some("Add an HTTPS RPC URL in settings to decode full tx bodies.".into()),
        seen_at: Utc::now().timestamp(),
    }
}

fn passes_filters(tx: &PendingTx, filters: &Filters, state: &AppState) -> bool {
    let watchlist = state.settings.read().watchlist.clone();
    let watchlist_hit = watchlist.iter().any(|w| {
        let a = w.address.to_lowercase();
        tx.from == a || tx.to.as_deref().unwrap_or("") == a
    });

    let eth_ok = filters.min_value_eth == 0.0 || tx.value_eth >= filters.min_value_eth;
    let usd_ok = filters.min_value_usd > 0.0
        && tx.value_usd.map(|v| v >= filters.min_value_usd).unwrap_or(false);

    if !watchlist_hit && !eth_ok && !usd_ok {
        return false;
    }

    if !filters.to_contracts.is_empty() {
        let to = tx.to.as_deref().unwrap_or("").to_lowercase();
        if !filters.to_contracts.iter().any(|c| c.to_lowercase() == to) {
            return false;
        }
    }
    if !filters.selectors.is_empty() {
        let sel = tx.selector.as_deref().unwrap_or("");
        if !filters.selectors.iter().any(|s| s.to_lowercase() == sel) {
            return false;
        }
    }
    true
}

/// Update the cached connection status and emit `mempool://status` to the
/// frontend. The payload includes both an English fallback `message` and a
/// localization `code`+`params` pair so the UI can render in the user's
/// preferred language.
fn update_status_keyed(
    app: &AppHandle,
    state: &AppState,
    connected: bool,
    code: &str,
    params: HashMap<String, String>,
) {
    let message = render_status_en(code, &params);
    let status = ConnectionStatus {
        connected,
        message: message.clone(),
        code: Some(code.to_string()),
        params: if params.is_empty() { None } else { Some(params) },
    };
    state.set_connection(status.clone());
    let _ = app.emit(EVENT_STATUS, &status);
}

/// English fallback rendering for a status `code`+`params` pair. Mirrors the
/// templates in the frontend i18n dictionary so the `message` field is
/// always usable on its own.
fn render_status_en(code: &str, params: &HashMap<String, String>) -> String {
    let g = |k: &str| params.get(k).cloned().unwrap_or_default();
    match code {
        "status.idle" => "Idle".to_string(),
        "status.no_rpc" => "No RPC WebSocket URL configured.".to_string(),
        "status.connecting" => format!("Connecting to {}", g("url")),
        "status.streaming" => "Streaming pending transactions".to_string(),
        "status.closed" => "Connection closed by server.".to_string(),
        "status.disconnected" => format!(
            "Disconnected: {}. Retrying in {}s.",
            g("err"),
            g("secs"),
        ),
        "status.stopped" => "Stopped".to_string(),
        _ => code.to_string(),
    }
}

/// Emit a status update without going through the worker. Used by
/// `stop_streaming` so the frontend status bar reflects the change immediately
/// after the user clicks Stop.
pub fn emit_status_keyed(
    app: &AppHandle,
    state: &AppState,
    connected: bool,
    code: &str,
    params: HashMap<String, String>,
) {
    update_status_keyed(app, state, connected, code, params);
}

/// Read messages off the websocket until either a pending-tx notification
/// arrives (returned to caller for processing) or the stream ends. Pings are
/// answered transparently. Subscription acks (top-level `result`, no `params`)
/// are skipped.
async fn wait_for_pending_notification(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> anyhow::Result<Option<Message>> {
    while let Some(msg) = ws.next().await {
        let msg = msg.map_err(|e| anyhow!(e))?;
        match &msg {
            Message::Text(text) => {
                let v: Value = serde_json::from_str(text).unwrap_or(Value::Null);
                if v.get("params").and_then(|p| p.get("result")).is_some() {
                    return Ok(Some(msg));
                }
                // Otherwise it's an ack or unrelated reply — keep waiting.
            }
            Message::Ping(p) => {
                ws.send(Message::Pong(p.clone())).await.ok();
            }
            Message::Close(_) => return Ok(None),
            _ => {}
        }
    }
    Ok(None)
}

fn redact(url: &str) -> String {
    // Hide the API key portion of the URL when logging.
    if let Some(idx) = url.rfind('/') {
        let (head, tail) = url.split_at(idx + 1);
        if tail.len() > 6 {
            return format!("{}{}…{}", head, &tail[..3], &tail[tail.len() - 3..]);
        }
    }
    url.to_string()
}

fn nonempty(s: &str) -> Option<&str> {
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}
