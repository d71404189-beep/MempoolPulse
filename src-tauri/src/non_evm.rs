//! Non-EVM chain workers. Each non-EVM network has a fundamentally different
//! mempool / transaction-streaming model, so each gets a dedicated worker:
//!
//! - **Bitcoin** has a real public mempool exposed by mempool.space's
//!   WebSocket. We subscribe to `track-mempool-block` style events and
//!   surface true *pending* transactions before they're mined.
//! - **Solana** has no traditional mempool (TPU forwards txs straight to the
//!   leader). The closest realtime stream is `logsSubscribe`, which delivers
//!   confirmed signatures within ~1-2s — close enough to feel live.
//! - **TRON** doesn't expose a mempool subscription on public nodes. We poll
//!   the latest block (3s blocktime) and emit each tx the first time we see
//!   it; this surfaces confirmed txs with ~3s latency.
//! - **TON** uses Toncenter v2 polling: every 5s we fetch the latest mainnet
//!   transactions (`getTransactions` on the workchain master) and emit new
//!   ones we haven't seen.
//! - **Sui** subscribes to `suix_subscribeTransaction` over Mysten's
//!   WebSocket and emits each confirmed transaction as it streams.
//!
//! All workers funnel into the same `PendingTx` event the EVM workers use,
//! so the frontend's live table doesn't need to know about the chain kind.
//! The `label` field carries the chain-specific human-readable summary
//! (e.g. "BTC transfer", "Solana logs", "TRON: triggerSmartContract",
//! "TON message", "Sui: 0x2::sui::SUI move call").

use crate::mempool::{redact, update_status_keyed, EVENT_PENDING};
use crate::state::AppState;
use crate::types::{ChainConfig, ChainKind, PendingTx};
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::Message;

/// Same reconnect/backoff pattern as the EVM worker, but dispatches by chain
/// kind to one of the implementations below.
pub async fn run_with_reconnect(app: AppHandle, state: AppState, chain: ChainConfig) {
    let mut backoff = Duration::from_secs(2);
    loop {
        if *state.shutdown.read() {
            return;
        }
        // Reload from settings each loop so URL/enabled edits propagate.
        let current = current_chain(&state, &chain.id).unwrap_or_else(|| chain.clone());
        if !current.enabled {
            update_status_keyed(&app, &state, &current, false, "status.idle", HashMap::new());
            return;
        }

        // Pick the URL the worker actually connects with so the empty-URL
        // guard and the "Connecting to …" status reflect reality. WS-based
        // kinds (Bitcoin, Solana, Sui) check rpc_ws_url; HTTP-polling kinds
        // (TRON, TON) check rpc_http_url.
        let primary_url = match current.kind {
            ChainKind::Bitcoin | ChainKind::Solana | ChainKind::Sui => current.rpc_ws_url.clone(),
            _ => current.rpc_http_url.clone(),
        };
        if primary_url.is_empty() {
            update_status_keyed(&app, &state, &current, false, "status.no_rpc", HashMap::new());
            tokio::time::sleep(Duration::from_secs(3)).await;
            continue;
        }
        let mut p = HashMap::new();
        p.insert("url".into(), redact(&primary_url));
        update_status_keyed(&app, &state, &current, false, "status.connecting", p);

        let res = match current.kind {
            ChainKind::Bitcoin => run_bitcoin(&app, &state, &current).await,
            ChainKind::Solana => run_solana(&app, &state, &current).await,
            ChainKind::Tron => run_tron(&app, &state, &current).await,
            ChainKind::Ton => run_ton(&app, &state, &current).await,
            ChainKind::Sui => run_sui(&app, &state, &current).await,
            ChainKind::Evm => unreachable!("EVM dispatched elsewhere"),
        };

        match res {
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

fn streaming_status(app: &AppHandle, state: &AppState, chain: &ChainConfig) {
    update_status_keyed(app, state, chain, true, "status.streaming", HashMap::new());
}

fn emit_tx(app: &AppHandle, tx: PendingTx) {
    let _ = app.emit(EVENT_PENDING, &tx);
}

fn now_millis() -> i64 {
    Utc::now().timestamp_millis()
}

// --------------------------------------------------------------------------
// Bitcoin — mempool.space WebSocket
// --------------------------------------------------------------------------

async fn run_bitcoin(app: &AppHandle, state: &AppState, chain: &ChainConfig) -> Result<()> {
    let req = chain.rpc_ws_url.as_str().into_client_request()?;
    let (mut ws, _) = tokio_tungstenite::connect_async(req)
        .await
        .context("connect mempool.space ws")?;

    // Mempool.space's protocol: send `{"action":"want","data":[...]}` listing
    // the streams we want. `mempool-blocks` gives us pending-block bundles,
    // `live-2h-chart` is just stats. We use `track-mempool-block` event from
    // the `mempool-blocks` stream which lists txs about to be mined.
    let subscribe = json!({
        "action": "want",
        "data": ["mempool-blocks", "live-2h-chart"]
    });
    ws.send(Message::Text(subscribe.to_string().into())).await?;

    streaming_status(app, state, chain);
    let price = state.prices.usd(&chain.coingecko_id).await;
    let mut last_price_at = std::time::Instant::now();
    let mut current_price = if price > 0.0 { Some(price) } else { None };
    let mut seen: HashSet<String> = HashSet::new();
    let mut order: VecDeque<String> = VecDeque::new();

    while let Some(msg) = ws.next().await {
        if *state.shutdown.read() {
            return Ok(());
        }
        let msg = msg?;
        let text = match msg {
            Message::Text(t) => t.to_string(),
            Message::Binary(b) => String::from_utf8_lossy(&b).to_string(),
            Message::Ping(p) => {
                ws.send(Message::Pong(p)).await.ok();
                continue;
            }
            Message::Close(_) => break,
            _ => continue,
        };
        let v: Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if last_price_at.elapsed() > Duration::from_secs(60) {
            let p = state.prices.usd(&chain.coingecko_id).await;
            if p > 0.0 {
                current_price = Some(p);
            }
            last_price_at = std::time::Instant::now();
        }

        // mempool.space delivers projected blocks under `mempool-blocks` —
        // each entry has `transactions` (recent inclusions). We also accept
        // their newer `track-mempool-block` payload shape.
        let blocks = v
            .get("mempool-blocks")
            .or_else(|| v.get("projected-mempool-blocks"))
            .or_else(|| v.get("track-mempool-block"));
        if let Some(blocks) = blocks {
            let arr = if blocks.is_array() {
                blocks.as_array().cloned().unwrap_or_default()
            } else {
                vec![blocks.clone()]
            };
            for blk in arr {
                if let Some(txs) = blk.get("transactions").and_then(|v| v.as_array()) {
                    for tx in txs {
                        if let Some(pt) = bitcoin_tx_to_pending(tx, chain, current_price) {
                            if seen.insert(pt.hash.clone()) {
                                order.push_back(pt.hash.clone());
                                if order.len() > 5_000 {
                                    if let Some(old) = order.pop_front() {
                                        seen.remove(&old);
                                    }
                                }
                                emit_tx(app, pt);
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn bitcoin_tx_to_pending(
    tx: &Value,
    chain: &ChainConfig,
    price_usd: Option<f64>,
) -> Option<PendingTx> {
    let txid = tx.get("txid").and_then(|v| v.as_str())?.to_string();
    // Sum of all output values in satoshis. mempool.space's `vsize`/`fee` tx
    // shape carries `value` per `vout`, but the projected-block shape only
    // has aggregate fields — fall back to the `value` field on the tx
    // itself when individual outputs aren't present.
    let total_sats: u64 = if let Some(vouts) = tx.get("vout").and_then(|v| v.as_array()) {
        vouts
            .iter()
            .filter_map(|o| o.get("value").and_then(|v| v.as_u64()))
            .sum()
    } else {
        tx.get("value").and_then(|v| v.as_u64()).unwrap_or(0)
    };
    let value_btc = total_sats as f64 / 100_000_000.0;
    let value_usd = price_usd.map(|p| value_btc * p);

    // Pull a representative input/output address (best-effort for a UTXO chain
    // — there can be many on either side; we show the first non-empty one
    // and hint at the rest in the summary).
    let from = tx
        .get("vin")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|i| i.get("prevout"))
        .and_then(|p| p.get("scriptpubkey_address"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let to = tx
        .get("vout")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|o| o.get("scriptpubkey_address"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let in_count = tx
        .get("vin")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    let out_count = tx
        .get("vout")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);

    Some(PendingTx {
        chain: chain.id.clone(),
        native_symbol: "BTC".into(),
        hash: txid,
        from,
        to,
        value_wei: total_sats.to_string(),
        value_native: value_btc,
        value_usd,
        gas_gwei: None,
        gas_limit: tx
            .get("fee")
            .and_then(|v| v.as_u64())
            .map(|f| f.to_string()),
        input: String::new(),
        selector: None,
        label: Some("BTC transfer".into()),
        summary: Some(format!("{in_count} in → {out_count} out")),
        seen_at: now_millis(),
    })
}

// --------------------------------------------------------------------------
// Solana — mainnet WebSocket logsSubscribe
// --------------------------------------------------------------------------

async fn run_solana(app: &AppHandle, state: &AppState, chain: &ChainConfig) -> Result<()> {
    let req = chain.rpc_ws_url.as_str().into_client_request()?;
    let (mut ws, _) = tokio_tungstenite::connect_async(req)
        .await
        .context("connect solana ws")?;

    // logsSubscribe with "all" commitment = confirmed — emits every log
    // signature on the network. Volume is high on mainnet (~2-3k txs/sec)
    // but our buffer cap and filters take care of that downstream.
    let sub = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "logsSubscribe",
        "params": ["all", {"commitment": "confirmed"}]
    });
    ws.send(Message::Text(sub.to_string().into())).await?;

    streaming_status(app, state, chain);
    let mut last_price_at = std::time::Instant::now();
    let mut current_price = state.prices.usd(&chain.coingecko_id).await;

    while let Some(msg) = ws.next().await {
        if *state.shutdown.read() {
            return Ok(());
        }
        let msg = msg?;
        let text = match msg {
            Message::Text(t) => t.to_string(),
            Message::Binary(b) => String::from_utf8_lossy(&b).to_string(),
            Message::Ping(p) => {
                ws.send(Message::Pong(p)).await.ok();
                continue;
            }
            Message::Close(_) => break,
            _ => continue,
        };

        if last_price_at.elapsed() > Duration::from_secs(60) {
            let p = state.prices.usd(&chain.coingecko_id).await;
            if p > 0.0 {
                current_price = p;
            }
            last_price_at = std::time::Instant::now();
        }

        let v: Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Notification shape:
        // {"jsonrpc":"2.0","method":"logsNotification","params":{"result":{"value":{...},"context":{"slot":...}}}}
        let result = v
            .pointer("/params/result/value")
            .or_else(|| v.pointer("/params/result"));
        let Some(result) = result else { continue };

        let signature = result
            .get("signature")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if signature.is_empty() {
            continue;
        }

        let logs: Vec<String> = result
            .get("logs")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|s| s.as_str())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();
        let err = result
            .get("err")
            .map(|e| !e.is_null())
            .unwrap_or(false);

        let label = solana_label_from_logs(&logs).unwrap_or_else(|| "Solana program".into());
        let summary = if err {
            Some("Failed".into())
        } else {
            logs.iter()
                .find(|l| l.starts_with("Program log:"))
                .map(|s| s.replace("Program log:", "").trim().to_string())
        };

        emit_tx(
            app,
            PendingTx {
                chain: chain.id.clone(),
                native_symbol: "SOL".into(),
                hash: signature,
                // Solana doesn't have a single "from" the way EVM does;
                // pulling the fee payer would require a follow-up
                // getTransaction call — skipped for stream throughput.
                from: String::new(),
                to: None,
                value_wei: "0".into(),
                value_native: 0.0,
                value_usd: if current_price > 0.0 { Some(0.0) } else { None },
                gas_gwei: None,
                gas_limit: None,
                input: String::new(),
                selector: None,
                label: Some(label),
                summary,
                seen_at: now_millis(),
            },
        );
    }
    Ok(())
}

fn solana_label_from_logs(logs: &[String]) -> Option<String> {
    // Best-effort program identification by the first `Program <pubkey> invoke`
    // line. Map well-known program ids to friendly names.
    for l in logs {
        if let Some(rest) = l.strip_prefix("Program ") {
            if let Some(idx) = rest.find(" invoke") {
                let pubkey = &rest[..idx];
                let pretty = match pubkey {
                    "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4" => "Jupiter v6",
                    "JUPyiwrYJFskUPiHa7hkeR8VUtAeFoSYbKedZNsDvCN" => "Jupiter Aggregator",
                    "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8" => "Raydium AMM",
                    "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK" => "Raydium CLMM",
                    "9W959DqEETiGZocYWCQPaJ6sBmUzgfxXfqGeTEdp3aQP" => "Orca",
                    "whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc" => "Orca Whirlpools",
                    "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P" => "Pump.fun",
                    "M2mx93ekt1fmXSVkTrUL9xVFHkmME8HTUi5Cyc5aF7K" => "Magic Eden v2",
                    "metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s" => "Metaplex Token Metadata",
                    "11111111111111111111111111111111" => "System program",
                    "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA" => "SPL Token",
                    "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb" => "Token-2022",
                    _ => return Some(format!("{}", short_pubkey(pubkey))),
                };
                return Some(pretty.into());
            }
        }
    }
    None
}

fn short_pubkey(pk: &str) -> String {
    if pk.len() < 12 {
        return pk.to_string();
    }
    format!("{}…{}", &pk[..4], &pk[pk.len() - 4..])
}

// --------------------------------------------------------------------------
// TRON — latest-block polling via JSON-RPC
// --------------------------------------------------------------------------

async fn run_tron(app: &AppHandle, state: &AppState, chain: &ChainConfig) -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?;
    streaming_status(app, state, chain);
    let mut last_seen_block: u64 = 0;
    let mut last_price_at = std::time::Instant::now();
    let mut current_price = state.prices.usd(&chain.coingecko_id).await;
    let url = format!("{}/wallet/getnowblock", chain.rpc_http_url.trim_end_matches('/'));

    loop {
        if *state.shutdown.read() {
            return Ok(());
        }
        if last_price_at.elapsed() > Duration::from_secs(60) {
            let p = state.prices.usd(&chain.coingecko_id).await;
            if p > 0.0 {
                current_price = p;
            }
            last_price_at = std::time::Instant::now();
        }
        // TRON public node: POST /wallet/getnowblock returns current block.
        let res = client.post(&url).body("{}").send().await?;
        let blk: Value = res.json().await?;
        let block_num = blk
            .pointer("/block_header/raw_data/number")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        if block_num > last_seen_block {
            last_seen_block = block_num;
            if let Some(txs) = blk.get("transactions").and_then(|v| v.as_array()) {
                for tx in txs {
                    if let Some(pt) = tron_tx_to_pending(tx, chain, current_price) {
                        emit_tx(app, pt);
                    }
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

fn tron_tx_to_pending(tx: &Value, chain: &ChainConfig, price_usd: f64) -> Option<PendingTx> {
    let txid = tx.get("txID").and_then(|v| v.as_str())?.to_string();
    let contract = tx
        .pointer("/raw_data/contract/0")
        .cloned()
        .unwrap_or(Value::Null);
    let contract_type = contract
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown")
        .to_string();
    let value_param = contract.pointer("/parameter/value").cloned().unwrap_or(Value::Null);
    let from_hex = value_param
        .get("owner_address")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let to_hex = value_param
        .get("to_address")
        .or_else(|| value_param.get("contract_address"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let amount = value_param
        .get("amount")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    // 1 TRX = 1e6 sun. Native value only meaningful for TransferContract.
    let value_native = if contract_type == "TransferContract" {
        amount as f64 / 1_000_000.0
    } else {
        0.0
    };
    let value_usd = if price_usd > 0.0 {
        Some(value_native * price_usd)
    } else {
        None
    };

    Some(PendingTx {
        chain: chain.id.clone(),
        native_symbol: "TRX".into(),
        hash: txid,
        from: from_hex,
        to: to_hex,
        value_wei: amount.to_string(),
        value_native,
        value_usd,
        gas_gwei: None,
        gas_limit: None,
        input: String::new(),
        selector: None,
        label: Some(format!("TRON: {contract_type}")),
        summary: None,
        seen_at: now_millis(),
    })
}

// --------------------------------------------------------------------------
// TON — Toncenter v3 polling
// --------------------------------------------------------------------------
//
// Toncenter v3 returns full transaction bodies (in_msg.value + decoded
// opcodes) in a single `/transactions` call, so we can emit native value
// and human-readable labels without N+1 follow-up requests. The default
// chain URL is `https://toncenter.com/api/v3`.

async fn run_ton(app: &AppHandle, state: &AppState, chain: &ChainConfig) -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?;
    streaming_status(app, state, chain);
    let mut last_lt: u64 = 0;
    let mut last_price_at = std::time::Instant::now();
    let mut current_price = state.prices.usd(&chain.coingecko_id).await;
    let base = chain.rpc_http_url.trim_end_matches('/');

    loop {
        if *state.shutdown.read() {
            return Ok(());
        }
        if last_price_at.elapsed() > Duration::from_secs(60) {
            let p = state.prices.usd(&chain.coingecko_id).await;
            if p > 0.0 {
                current_price = p;
            }
            last_price_at = std::time::Instant::now();
        }

        // Pull the most recent transactions. We sort desc, so the first item
        // is freshest; we walk the page and keep emitting until we hit our
        // last_lt cursor.
        let url = format!("{}/transactions?limit=50&sort=desc", base);
        let res = client.get(&url).send().await?;
        if !res.status().is_success() {
            return Err(anyhow!("toncenter: HTTP {}", res.status()));
        }
        let body: Value = res.json().await?;
        if let Some(arr) = body.get("transactions").and_then(|v| v.as_array()) {
            let mut max_lt = last_lt;
            for t in arr {
                let lt = t
                    .get("lt")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(0);
                if lt <= last_lt {
                    continue;
                }
                if last_lt > 0 {
                    if let Some(pt) = ton_tx_to_pending(t, chain, current_price) {
                        emit_tx(app, pt);
                    }
                }
                if lt > max_lt {
                    max_lt = lt;
                }
            }
            last_lt = max_lt;
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

fn ton_tx_to_pending(t: &Value, chain: &ChainConfig, price_usd: f64) -> Option<PendingTx> {
    let hash = t.get("hash").and_then(|v| v.as_str())?.to_string();
    let account = t
        .get("account")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let in_msg = t.get("in_msg").cloned().unwrap_or(Value::Null);
    let from = in_msg
        .get("source")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let to = in_msg
        .get("destination")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            // Fall back to first out_msg destination for tx without an in_msg
            // (e.g. an external-out contract emission).
            t.pointer("/out_msgs/0/destination")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        });

    let value_nano: u128 = in_msg
        .get("value")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<u128>().ok())
        .unwrap_or_else(|| in_msg.get("value").and_then(|v| v.as_u64()).unwrap_or(0) as u128);
    let value_native = value_nano as f64 / 1_000_000_000.0;
    let value_usd = if price_usd > 0.0 {
        Some(value_native * price_usd)
    } else {
        None
    };

    // v3 sometimes pre-decodes well-known opcodes; fall back to body opcode
    // hex matching when it doesn't.
    let label = ton_label(&in_msg);

    let summary = in_msg
        .get("decoded_body")
        .and_then(|b| b.get("text"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Some(PendingTx {
        chain: chain.id.clone(),
        native_symbol: "TON".into(),
        hash,
        from: if from.is_empty() { account } else { from },
        to,
        value_wei: value_nano.to_string(),
        value_native,
        value_usd,
        gas_gwei: None,
        gas_limit: None,
        input: String::new(),
        selector: None,
        label: Some(label),
        summary,
        seen_at: now_millis(),
    })
}

fn ton_label(in_msg: &Value) -> String {
    // Friendly name from toncenter v3's decoder when present.
    if let Some(op) = in_msg.get("decoded_op_name").and_then(|v| v.as_str()) {
        return match op {
            "jetton_transfer" => "TON: Jetton transfer",
            "jetton_internal_transfer" => "TON: Jetton internal",
            "jetton_burn" => "TON: Jetton burn",
            "nft_transfer" => "TON: NFT transfer",
            "nft_ownership_assigned" => "TON: NFT received",
            "stonfi_swap" => "TON: STON.fi swap",
            "stonfi_swap_v2" => "TON: STON.fi v2 swap",
            "dedust_swap" => "TON: DeDust swap",
            "dedust_swap_external" => "TON: DeDust swap",
            "wallet_signed_internal_v5" => "TON: Wallet v5",
            "wallet_signed_external_v5" => "TON: Wallet v5",
            "telegram_payments" => "TON: Telegram payment",
            other => return format!("TON: {other}"),
        }
        .into();
    }
    // Fall back to the raw 32-bit opcode in body_hash-prefix form.
    if let Some(op_hex) = in_msg.get("opcode").and_then(|v| v.as_str()) {
        return match op_hex {
            "0x00000000" | "" => "TON transfer".into(),
            "0x0f8a7ea5" => "TON: Jetton transfer".into(),
            "0x178d4519" => "TON: Jetton internal".into(),
            "0x595f07bc" => "TON: Jetton burn".into(),
            "0x05138d91" => "TON: NFT transfer".into(),
            "0x05fcd673" => "TON: NFT ownership".into(),
            "0x25938561" => "TON: STON.fi swap".into(),
            "0xe3a0d482" => "TON: DeDust swap".into(),
            other => format!("TON: op {other}"),
        };
    }
    // No decoded op + no opcode = simple value-only transfer.
    "TON transfer".into()
}

// --------------------------------------------------------------------------
// Sui — Mysten WebSocket subscribeTransaction + bounded follow-up fetch
// --------------------------------------------------------------------------
//
// `suix_subscribeTransaction` only returns the digest + sender; balance
// changes and friendly Move-call labels live in `sui_getTransactionBlock`.
// We do a follow-up RPC per digest with bounded concurrency (semaphore)
// so the worker never has more than N in-flight enrichment calls. If the
// follow-up fails (rate limit, 5xx, etc.) we fall back to emitting the
// minimal record so the user still sees the activity.

const SUI_ENRICH_CONCURRENCY: usize = 6;
const SUI_COIN_TYPE: &str = "0x2::sui::SUI";

async fn run_sui(app: &AppHandle, state: &AppState, chain: &ChainConfig) -> Result<()> {
    let req = chain.rpc_ws_url.as_str().into_client_request()?;
    let (mut ws, _) = tokio_tungstenite::connect_async(req)
        .await
        .context("connect sui ws")?;

    let sub = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "suix_subscribeTransaction",
        "params": [{"All": []}]
    });
    ws.send(Message::Text(sub.to_string().into())).await?;
    streaming_status(app, state, chain);

    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()?;
    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(SUI_ENRICH_CONCURRENCY));

    while let Some(msg) = ws.next().await {
        if *state.shutdown.read() {
            return Ok(());
        }
        let msg = msg?;
        let text = match msg {
            Message::Text(t) => t.to_string(),
            Message::Binary(b) => String::from_utf8_lossy(&b).to_string(),
            Message::Ping(p) => {
                ws.send(Message::Pong(p)).await.ok();
                continue;
            }
            Message::Close(_) => break,
            _ => continue,
        };
        let v: Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let Some(result) = v.pointer("/params/result") else {
            continue;
        };
        let digest = result
            .get("digest")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if digest.is_empty() {
            continue;
        }
        let sender_hint = result
            .pointer("/transaction/data/sender")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // Enrich asynchronously so a slow upstream doesn't stall the WS pump.
        let app_c = app.clone();
        let state_c = state.clone();
        let chain_c = chain.clone();
        let client_c = http_client.clone();
        let sem = semaphore.clone();
        tokio::spawn(async move {
            let _permit = match sem.acquire_owned().await {
                Ok(p) => p,
                Err(_) => return,
            };
            let pt = sui_enrich(&client_c, &chain_c, &state_c, &digest, &sender_hint).await;
            emit_tx(&app_c, pt);
        });
    }
    Ok(())
}

async fn sui_enrich(
    client: &reqwest::Client,
    chain: &ChainConfig,
    state: &AppState,
    digest: &str,
    sender_hint: &str,
) -> PendingTx {
    let price = state.prices.usd(&chain.coingecko_id).await;
    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "sui_getTransactionBlock",
        "params": [
            digest,
            {
                "showInput": true,
                "showBalanceChanges": true,
                "showEffects": true
            }
        ]
    });

    let resp = client
        .post(chain.rpc_http_url.trim_end_matches('/'))
        .json(&body)
        .send()
        .await;

    let detail = match resp {
        Ok(r) if r.status().is_success() => r.json::<Value>().await.ok(),
        _ => None,
    };

    let detail = detail.unwrap_or(Value::Null);
    let result = detail.get("result").cloned().unwrap_or(Value::Null);

    let sender = result
        .pointer("/transaction/data/sender")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| sender_hint.to_string());

    // Find SUI delta on the sender. We pick the largest absolute outgoing
    // amount (negative), which corresponds to "what was actually moved out".
    let mut value_mist: i128 = 0;
    let mut to_address: Option<String> = None;
    if let Some(arr) = result.get("balanceChanges").and_then(|v| v.as_array()) {
        let mut sender_outflow: i128 = 0;
        let mut largest_recipient: Option<(i128, String)> = None;
        for c in arr {
            let coin = c
                .get("coinType")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if coin != SUI_COIN_TYPE {
                continue;
            }
            let amount: i128 = c
                .get("amount")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<i128>().ok())
                .unwrap_or(0);
            // owner can be {AddressOwner: "0x..."} | {ObjectOwner: ...} | "Immutable" | {Shared: ...}
            let owner = c
                .pointer("/owner/AddressOwner")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            if owner.as_deref() == Some(sender.as_str()) {
                if amount < sender_outflow {
                    sender_outflow = amount;
                }
            } else if amount > 0 {
                if let Some(addr) = owner {
                    let cur = largest_recipient
                        .as_ref()
                        .map(|(a, _)| *a)
                        .unwrap_or(0);
                    if amount > cur {
                        largest_recipient = Some((amount, addr));
                    }
                }
            }
        }
        value_mist = -sender_outflow;
        to_address = largest_recipient.map(|(_, a)| a);
    }
    // 1 SUI = 1e9 mist
    let value_native = value_mist as f64 / 1_000_000_000.0;
    let value_usd = if price > 0.0 && value_native > 0.0 {
        Some(value_native * price)
    } else {
        None
    };

    let label = sui_label(&result);

    PendingTx {
        chain: chain.id.clone(),
        native_symbol: "SUI".into(),
        hash: digest.to_string(),
        from: sender,
        to: to_address,
        value_wei: value_mist.max(0).to_string(),
        value_native,
        value_usd,
        gas_gwei: None,
        gas_limit: result
            .pointer("/effects/gasUsed/computationCost")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        input: String::new(),
        selector: None,
        label: Some(label),
        summary: None,
        seen_at: now_millis(),
    }
}

fn sui_label(result: &Value) -> String {
    // ProgrammableTransaction → look at the first MoveCall, if any, and
    // map well-known package ids to friendly names.
    let pt = result
        .pointer("/transaction/data/transaction")
        .cloned()
        .unwrap_or(Value::Null);
    let kind = pt
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("ProgrammableTransaction");

    if let Some(txs) = pt.get("transactions").and_then(|v| v.as_array()) {
        for cmd in txs {
            if let Some(call) = cmd.get("MoveCall") {
                let pkg = call.get("package").and_then(|v| v.as_str()).unwrap_or("");
                let module = call.get("module").and_then(|v| v.as_str()).unwrap_or("");
                let function = call
                    .get("function")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let pretty = sui_known_package(pkg);
                if !pretty.is_empty() {
                    return format!("Sui: {pretty} {function}").trim().into();
                }
                if !module.is_empty() {
                    return format!("Sui: {module}::{function}");
                }
            }
            if cmd.get("TransferObjects").is_some() {
                return "Sui: TransferObjects".into();
            }
            if cmd.get("SplitCoins").is_some() {
                return "Sui: SplitCoins".into();
            }
        }
    }
    if kind == "ProgrammableTransaction" {
        return "Sui: ProgrammableTransaction".into();
    }
    format!("Sui: {kind}")
}

fn sui_known_package(pkg: &str) -> &'static str {
    match pkg {
        // Cetus AMM mainnet routers.
        "0x1eabed72c53feb3805120a081dc15963c204dc8d091542592abaf7a35689b2fb" => "Cetus AMM",
        "0x2eabed72c53feb3805120a081dc15963c204dc8d091542592abaf7a35689b2fb" => "Cetus Router",
        // Bluefin Spot/Perps (well-known mainnet ids).
        "0x3492c874c1e3b3e2984e8c41b589e642d4d0a5d6459e5a9cfc2d52fd7c89c267" => "Bluefin",
        // DeepBook v2/v3.
        "0x000000000000000000000000000000000000000000000000000000000000dee9" => "DeepBook",
        // Suilend lending.
        "0xf95b06141ed4a174f239417323bde3f209b972f5930d8521ea38a52aff3a6ddf" => "Suilend",
        // Aftermath finance router.
        "0xefe170ec0be4d762196bedecd7a065816576198a6527c99282a2551aaa7da38c" => "Aftermath",
        _ => "",
    }
}
