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
// TON — Toncenter v2 polling with getBlockTransactionsExt for native value
// --------------------------------------------------------------------------

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

        let url = format!("{}/getMasterchainInfo", base);
        let info: Value = match client.get(&url).send().await {
            Ok(r) => match r.json().await {
                Ok(v) => v,
                Err(_) => { tokio::time::sleep(Duration::from_secs(5)).await; continue; }
            },
            Err(_) => { tokio::time::sleep(Duration::from_secs(5)).await; continue; }
        };
        let last_seqno = match info.pointer("/result/last/seqno").and_then(|v| v.as_u64()) {
            Some(s) => s,
            None => { tokio::time::sleep(Duration::from_secs(5)).await; continue; }
        };
        let workchain = -1i32;
        let shard = "-9223372036854775808";

        // v1.6.1: Use getBlockTransactionsExt which returns full tx details
        // including in_msg.value (nanotons) — no extra RPC call needed.
        let txs_url = format!(
            "{}/getBlockTransactionsExt?workchain={}&shard={}&seqno={}&count=40",
            base, workchain, shard, last_seqno
        );
        let blk: Value = match client.get(&txs_url).send().await {
            Ok(r) => match r.json().await {
                Ok(v) => v,
                Err(_) => { tokio::time::sleep(Duration::from_secs(5)).await; continue; }
            },
            Err(_) => { tokio::time::sleep(Duration::from_secs(5)).await; continue; }
        };

        if let Some(arr) = blk.pointer("/result/transactions").and_then(|v| v.as_array()) {
            for t in arr {
                let lt = t
                    .get("transaction_id")
                    .and_then(|v| v.get("lt"))
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse::<u64>().ok())
                    .or_else(|| t.get("lt").and_then(|v| v.as_u64()))
                    .unwrap_or(0);
                if lt <= last_lt {
                    continue;
                }
                if last_lt > 0 {
                    if let Some(pt) = ton_tx_to_pending_ext(t, chain, current_price) {
                        emit_tx(app, pt);
                    }
                }
                if lt > last_lt {
                    last_lt = lt;
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

/// Decode TON opcode → human-readable label.
/// Covers standard jetton/NFT/DEX opcodes used by the TON ecosystem.
fn ton_decode_opcode(opcode: u32) -> Option<&'static str> {
    match opcode {
        0x0f8a7ea5 => Some("Jetton transfer"),
        0x178d4519 => Some("Jetton transfer notification"),
        0x7362d09c => Some("Jetton transfer notification (alt)"),
        0x595f07bc => Some("Jetton burn"),
        0x5fcc3d14 => Some("NFT transfer"),
        0x693d3950 => Some("NFT get static data"),
        0x2fcb26a2 => Some("NFT get static data response"),
        0x05138d91 => Some("NFT ownership assigned"),
        0x5b0f2bc => Some("Getgems NFT sale"),
        0x25938561 => Some("STON.fi swap"),
        0xfcf9e58f => Some("STON.fi provide liquidity"),
        0x6664de2a => Some("DeDust swap"),
        0xd55e4686 => Some("DeDust deposit"),
        0x0000000 => Some("TON transfer"),
        _ => None,
    }
}

/// Build a PendingTx from a getBlockTransactionsExt response entry.
/// Extracts in_msg.value for native TON amount and decodes message opcode.
fn ton_tx_to_pending_ext(t: &Value, chain: &ChainConfig, price_usd: f64) -> Option<PendingTx> {
    let hash = t
        .get("transaction_id")
        .and_then(|v| v.get("hash"))
        .and_then(|v| v.as_str())
        .or_else(|| t.get("hash").and_then(|v| v.as_str()))?
        .to_string();

    // account = "from" address in user-friendly base64 format
    let from = t
        .get("account")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // in_msg carries the incoming message with value and destination
    let in_msg = t.get("in_msg").cloned().unwrap_or(Value::Null);

    // Destination address from in_msg.destination
    let to = in_msg
        .get("destination")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    // Native value: in_msg.value is nanotons (string)
    let nanotons: u64 = in_msg
        .get("value")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .or_else(|| in_msg.get("value").and_then(|v| v.as_u64()))
        .unwrap_or(0);
    let value_ton = nanotons as f64 / 1_000_000_000.0;
    let value_usd = if price_usd > 0.0 && value_ton > 0.0 {
        Some(value_ton * price_usd)
    } else {
        None
    };

    // Decode message body opcode for human-readable label
    let msg_body = in_msg.get("msg_data").or_else(|| in_msg.get("body"));
    let opcode: Option<u32> = msg_body
        .and_then(|b| b.get("body"))
        .and_then(|v| v.as_str())
        .and_then(|hex| {
            let hex = hex.trim_start_matches("0x");
            if hex.len() >= 8 {
                u32::from_str_radix(&hex[..8], 16).ok()
            } else {
                None
            }
        });

    let label = opcode
        .and_then(ton_decode_opcode)
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            // Fallback: classify by msg_type
            let msg_type = in_msg.get("msg_type").and_then(|v| v.as_str()).unwrap_or("");
            match msg_type {
                "ext_in_msg_info" => "TON external call".to_string(),
                "int_msg_info" if nanotons > 0 => "TON transfer".to_string(),
                _ => "TON message".to_string(),
            }
        });

    Some(PendingTx {
        chain: chain.id.clone(),
        native_symbol: "TON".into(),
        hash,
        from,
        to,
        value_wei: nanotons.to_string(),
        value_native: value_ton,
        value_usd,
        gas_gwei: None,
        gas_limit: None,
        input: String::new(),
        selector: None,
        label: Some(label),
        summary: None,
        seen_at: now_millis(),
    })
}

// --------------------------------------------------------------------------
// Sui — Mysten WebSocket subscribeTransaction + follow-up for balance changes
// --------------------------------------------------------------------------

/// Map well-known Sui Move modules to human-readable labels.
fn sui_label_from_kind(tx_kind: &str, modules: &[String]) -> String {
    // First try to identify by well-known module names
    for m in modules {
        let label = match m.as_str() {
            s if s.contains("0x2::sui") => "SUI transfer",
            s if s.contains("cetus") => "Cetus AMM swap",
            s if s.contains("deepbook") => "DeepBook order",
            s if s.contains("bluefin") => "Bluefin perps",
            s if s.contains("navi") => "Navi lending",
            s if s.contains("turbos") => "Turbos DEX",
            s if s.contains("aftermath") => "Aftermath DEX",
            s if s.contains("staking") || s.contains("validator") => "Sui staking",
            s if s.contains("kiosk") => "Kiosk NFT",
            s if s.contains("coin") => "Coin operation",
            _ => continue,
        };
        return label.to_string();
    }
    // Fallback to tx kind
    match tx_kind {
        "ProgrammableTransaction" => "Sui programmable tx".to_string(),
        "ChangeEpoch" => "Sui epoch change".to_string(),
        "Genesis" => "Sui genesis".to_string(),
        other => format!("Sui: {other}"),
    }
}

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
    let mut last_price_at = std::time::Instant::now();
    let mut current_price = state.prices.usd(&chain.coingecko_id).await;

    // HTTP client for follow-up sui_getTransactionBlock calls
    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;
    let http_url = chain.rpc_http_url.clone();

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

        // Extract sender from subscription result
        let sender = result
            .pointer("/transaction/data/sender")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let tx_kind = result
            .pointer("/transaction/data/transaction/kind")
            .and_then(|v| v.as_str())
            .unwrap_or("ProgrammableTransaction")
            .to_string();

        // v1.6.1: Follow-up call to get balance changes and Move modules
        // Use showBalanceChanges + showInput to decode value and label
        let (value_sui, value_usd_opt, label, to_addr) = if !http_url.is_empty() {
            let body = json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "sui_getTransactionBlock",
                "params": [
                    digest,
                    {
                        "showInput": true,
                        "showBalanceChanges": true,
                        "showEffects": false,
                        "showEvents": false,
                        "showObjectChanges": false
                    }
                ]
            });
            match http_client.post(&http_url).json(&body).send().await {
                Ok(resp) => {
                    if let Ok(data) = resp.json::<Value>().await {
                        let tx_result = data.get("result").cloned().unwrap_or(Value::Null);

                        // Extract balance changes to find SUI amount and recipient
                        let mut max_sui_change: f64 = 0.0;
                        let mut recipient: Option<String> = None;
                        if let Some(changes) = tx_result.get("balanceChanges").and_then(|v| v.as_array()) {
                            for change in changes {
                                let coin_type = change.get("coinType").and_then(|v| v.as_str()).unwrap_or("");
                                if coin_type.contains("0x2::sui::SUI") {
                                    let amount_str = change.get("amount").and_then(|v| v.as_str()).unwrap_or("0");
                                    let amount: i64 = amount_str.parse().unwrap_or(0);
                                    // Positive change = receiver
                                    if amount > 0 {
                                        let sui_val = amount as f64 / 1_000_000_000.0;
                                        if sui_val > max_sui_change {
                                            max_sui_change = sui_val;
                                            recipient = change.get("owner")
                                                .and_then(|o| o.get("AddressOwner"))
                                                .and_then(|v| v.as_str())
                                                .map(|s| s.to_string());
                                        }
                                    }
                                }
                            }
                        }

                        // Extract Move module names for label decoding
                        let mut modules: Vec<String> = Vec::new();
                        if let Some(txs) = tx_result.pointer("/transaction/data/transaction/transactions").and_then(|v| v.as_array()) {
                            for tx_call in txs {
                                if let Some(mv) = tx_call.get("MoveCall") {
                                    let pkg = mv.get("package").and_then(|v| v.as_str()).unwrap_or("");
                                    let module = mv.get("module").and_then(|v| v.as_str()).unwrap_or("");
                                    let func = mv.get("function").and_then(|v| v.as_str()).unwrap_or("");
                                    modules.push(format!("{}::{}::{}", pkg, module, func));
                                }
                            }
                        }

                        let label = sui_label_from_kind(&tx_kind, &modules);
                        let usd = if current_price > 0.0 && max_sui_change > 0.0 {
                            Some(max_sui_change * current_price)
                        } else {
                            None
                        };
                        (max_sui_change, usd, label, recipient)
                    } else {
                        (0.0, None, format!("Sui: {tx_kind}"), None)
                    }
                }
                Err(_) => (0.0, None, format!("Sui: {tx_kind}"), None),
            }
        } else {
            (0.0, None, format!("Sui: {tx_kind}"), None)
        };

        let mist = (value_sui * 1_000_000_000.0) as u64;

        emit_tx(
            app,
            PendingTx {
                chain: chain.id.clone(),
                native_symbol: "SUI".into(),
                hash: digest,
                from: sender,
                to: to_addr,
                value_wei: mist.to_string(),
                value_native: value_sui,
                value_usd: value_usd_opt,
                gas_gwei: None,
                gas_limit: None,
                input: String::new(),
                selector: None,
                label: Some(label),
                summary: None,
                seen_at: now_millis(),
            },
        );
    }
    Ok(())
}
