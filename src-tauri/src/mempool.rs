//! Mempool subscription workers (one per enabled EVM chain).
//!
//! Each worker connects to a user-supplied JSON-RPC WebSocket and streams
//! pending transactions to the frontend as Tauri events tagged with the chain
//! id. We try the richer `alchemy_pendingTransactions` subscription first
//! (returns full tx bodies); on providers that don't support it (e.g.
//! publicnode) we fall back to `eth_subscribe newPendingTransactions` (hashes
//! only) and enrich each tx with a follow-up `eth_getTransactionByHash` over
//! HTTP.

use crate::decoder;
use crate::state::AppState;
use crate::types::{ChainConfig, ConnectionStatus, Filters, PendingTx};
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

/// How long to wait for the first pending-tx notification after an
/// `alchemy_pendingTransactions` subscription is acknowledged. Some providers
/// (e.g. publicnode) silently accept the subscribe call and return an id but
/// never push notifications — if no notification arrives within this window
/// we unsubscribe and fall back to the standard `newPendingTransactions`.
const ALCHEMY_PROBE_SECS: u64 = 7;
const SUBSCRIBE_ACK_SECS: u64 = 5;

/// Cancel all running workers and spawn a fresh one for every enabled chain.
pub async fn restart(app: AppHandle, state: AppState) {
    {
        let mut guard = state.workers.lock().await;
        *state.shutdown.write() = true;
        for (_, handle) in guard.drain() {
            handle.abort();
        }
        *state.shutdown.write() = false;
    }

    let chains = { state.settings.read().chains.clone() };
    for chain in chains {
        if !chain.enabled {
            // Reset to idle in case it was previously streaming.
            update_status_keyed(
                &app,
                &state,
                &chain,
                false,
                "status.idle",
                HashMap::new(),
            );
            continue;
        }
        let app_clone = app.clone();
        let state_clone = state.clone();
        let chain_id = chain.id.clone();
        let handle = tokio::spawn(async move {
            run_with_reconnect(app_clone, state_clone, chain).await;
        });
        state.workers.lock().await.insert(chain_id, handle);
    }
}

async fn run_with_reconnect(app: AppHandle, state: AppState, chain: ChainConfig) {
    let mut backoff = Duration::from_secs(2);
    loop {
        if *state.shutdown.read() {
            return;
        }

        // The chain config can be edited at runtime; re-read each loop so the
        // worker picks up URL changes without a full restart.
        let current = current_chain(&state, &chain.id).unwrap_or_else(|| chain.clone());
        if !current.enabled {
            update_status_keyed(&app, &state, &current, false, "status.idle", HashMap::new());
            return;
        }
        if current.rpc_ws_url.is_empty() {
            update_status_keyed(&app, &state, &current, false, "status.no_rpc", HashMap::new());
            tokio::time::sleep(Duration::from_secs(3)).await;
            continue;
        }

        let mut p = HashMap::new();
        p.insert("url".into(), redact(&current.rpc_ws_url));
        update_status_keyed(&app, &state, &current, false, "status.connecting", p);

        match run_once(&app, &state, &current).await {
            Ok(()) => {
                update_status_keyed(
                    &app,
                    &state,
                    &current,
                    false,
                    "status.closed",
                    HashMap::new(),
                );
                tokio::time::sleep(Duration::from_secs(2)).await;
                backoff = Duration::from_secs(2);
            }
            Err(e) => {
                let mut p = HashMap::new();
                p.insert("err".into(), e.to_string());
                p.insert("secs".into(), backoff.as_secs().to_string());
                update_status_keyed(&app, &state, &current, false, "status.disconnected", p);
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(30));
            }
        }
    }
}

fn current_chain(state: &AppState, id: &str) -> Option<ChainConfig> {
    state
        .settings
        .read()
        .chains
        .iter()
        .find(|c| c.id == id)
        .cloned()
}

async fn run_once(app: &AppHandle, state: &AppState, chain: &ChainConfig) -> anyhow::Result<()> {
    let request = chain
        .rpc_ws_url
        .as_str()
        .into_client_request()
        .with_context(|| format!("Invalid WebSocket URL: {}", chain.rpc_ws_url))?;
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
    if let Ok(Some(Ok(Message::Text(text)))) =
        tokio::time::timeout(Duration::from_secs(SUBSCRIBE_ACK_SECS), ws.next()).await
    {
        let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        if let Some(id) = v.get("result").and_then(|r| r.as_str()) {
            alchemy_sub_id = Some(id.to_string());
        }
    }

    // Probe for an actual notification — some providers ack the subscribe call
    // but never push. Any notification received during the probe is buffered
    // and replayed into the main loop.
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

    update_status_keyed(app, state, chain, true, "status.streaming", HashMap::new());

    let http_url = chain.rpc_http_url.clone();
    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .ok();

    // Application-level keepalive: many public RPC providers (publicnode,
    // free-tier Alchemy / QuickNode, anything behind an idle-timeout reverse
    // proxy) close WS connections that go quiet for ~30-60s, even when the
    // subscription itself is healthy. We send a tungstenite ping every 25s so
    // the underlying TCP connection always has recent activity.
    let mut keepalive = tokio::time::interval(Duration::from_secs(25));
    // Skip the immediate first tick — the connection was just opened.
    keepalive.tick().await;

    loop {
        let next = if let Some(m) = buffered.take() {
            Some(Ok(m))
        } else {
            tokio::select! {
                m = ws.next() => m,
                _ = keepalive.tick() => {
                    if ws.send(Message::Ping(Default::default())).await.is_err() {
                        return Err(anyhow!("WebSocket keepalive ping failed"));
                    }
                    continue;
                }
            }
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

        let pending_tx = if params.is_object() {
            build_pending_tx(&params, state, chain).await
        } else if let Some(hash) = params.as_str() {
            if let (Some(client), Some(http_url)) = (http_client.as_ref(), nonempty(&http_url)) {
                if let Some(tx) = fetch_tx_by_hash(client, http_url, hash).await {
                    build_pending_tx(&tx, state, chain).await
                } else {
                    continue;
                }
            } else {
                Some(stub_from_hash(hash, chain))
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

async fn build_pending_tx(
    raw: &Value,
    state: &AppState,
    chain: &ChainConfig,
) -> Option<PendingTx> {
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
    let value_native = value_wei_f / 1e18;

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
    // Plain native-asset transfers (no calldata) get a friendly chain-specific
    // label like "ETH transfer" / "BNB transfer" instead of falling through to
    // the generic "raw call" placeholder.
    let (label, summary) = if input.is_empty() || input.eq_ignore_ascii_case("0x") {
        if value_native > 0.0 {
            (Some(format!("{} transfer", chain.native_symbol)), None)
        } else {
            (None, None)
        }
    } else {
        decoder::decode(&input)
    };

    let native_usd = state.prices.usd(&chain.coingecko_id).await;
    let value_usd = if native_usd > 0.0 {
        Some(value_native * native_usd)
    } else {
        None
    };

    Some(PendingTx {
        chain: chain.id.clone(),
        native_symbol: chain.native_symbol.clone(),
        hash,
        from,
        to,
        value_wei,
        value_native,
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

fn stub_from_hash(hash: &str, chain: &ChainConfig) -> PendingTx {
    PendingTx {
        chain: chain.id.clone(),
        native_symbol: chain.native_symbol.clone(),
        hash: hash.to_string(),
        from: String::new(),
        to: None,
        value_wei: "0".into(),
        value_native: 0.0,
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

    let native_ok = filters.min_value_eth == 0.0 || tx.value_native >= filters.min_value_eth;
    let usd_ok = filters.min_value_usd > 0.0
        && tx.value_usd.map(|v| v >= filters.min_value_usd).unwrap_or(false);

    if !watchlist_hit && !native_ok && !usd_ok {
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

/// Update the cached connection status for a chain and emit `mempool://status`
/// to the frontend.
fn update_status_keyed(
    app: &AppHandle,
    state: &AppState,
    chain: &ChainConfig,
    connected: bool,
    code: &str,
    params: HashMap<String, String>,
) {
    let message = render_status_en(code, &params);
    let status = ConnectionStatus {
        chain: chain.id.clone(),
        connected,
        message,
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

/// Mark every chain as stopped and emit a status event. Used by
/// `stop_streaming` so the frontend status bar reflects the change immediately
/// after the user clicks Stop.
pub fn emit_all_stopped(app: &AppHandle, state: &AppState) {
    let chains = { state.settings.read().chains.clone() };
    for chain in chains {
        update_status_keyed(app, state, &chain, false, "status.stopped", HashMap::new());
    }
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
