//! Spawns a forked anvil instance, replays a pending transaction against it,
//! and returns a structured simulation result (status, gas used, decoded
//! ERC20 transfers).
//!
//! Anvil is started with `--port 0` so it picks a free port; we read the
//! "Listening on 127.0.0.1:<port>" line from stdout to discover it. We then
//! talk to anvil over plain HTTP JSON-RPC — impersonating the original
//! sender, sending the original transaction payload, fetching the receipt,
//! and decoding ERC20 Transfer logs into a human-readable list. The child
//! process is always killed on drop / on the way out so we never leak anvil
//! instances.

use crate::anvil;
use crate::types::ChainConfig;
use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::process::Stdio;
use std::time::Duration;
use tauri::AppHandle;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::time::timeout;

/// Result of a successful simulation. `success: false` means the tx reverted
/// — `revert_reason` may carry a decoded message if anvil exposed one.
#[derive(Debug, Clone, Serialize)]
pub struct SimulationResult {
    pub success: bool,
    pub gas_used: u64,
    pub effective_gas_price_gwei: f64,
    pub revert_reason: Option<String>,
    pub transfers: Vec<DecodedTransfer>,
    pub log_count: usize,
    pub block_number: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DecodedTransfer {
    pub token: String,
    pub from: String,
    pub to: String,
    /// Raw uint256 value as decimal string (we don't know decimals here).
    pub value_raw: String,
}

/// Holds the running anvil subprocess so we kill it on drop even when the
/// simulation flow returns early via `?`.
struct AnvilGuard {
    child: Option<Child>,
}

impl AnvilGuard {
    async fn shutdown(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill().await;
        }
    }
}

impl Drop for AnvilGuard {
    fn drop(&mut self) {
        if let Some(mut c) = self.child.take() {
            // Best-effort sync kill if drop happens off a tokio runtime.
            let _ = c.start_kill();
        }
    }
}

pub async fn simulate(app: AppHandle, chain: ChainConfig, tx_hash: String) -> Result<SimulationResult> {
    if chain.rpc_http_url.trim().is_empty() {
        return Err(anyhow!(
            "Simulation requires the chain's HTTPS RPC URL to be set in Settings."
        ));
    }
    let anvil_bin = anvil::ensure(app).await?;

    // Step 1: pull the original tx so we know exactly what to replay.
    let tx = fetch_tx(&chain.rpc_http_url, &tx_hash)
        .await
        .with_context(|| "Could not fetch the pending transaction from the upstream RPC")?;
    let block_number = fetch_block_number(&chain.rpc_http_url).await.unwrap_or(0);

    // Step 2: spawn anvil in fork mode and discover its port.
    let mut child = Command::new(&anvil_bin)
        .args([
            "--fork-url",
            chain.rpc_http_url.as_str(),
            "--port",
            "0",
            "--silent",
            "--no-mining",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("Failed to spawn anvil at {}", anvil_bin.display()))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("anvil stdout unavailable"))?;
    let mut guard = AnvilGuard { child: Some(child) };

    let port = match timeout(Duration::from_secs(30), wait_for_port(stdout)).await {
        Ok(Ok(p)) => p,
        Ok(Err(e)) => {
            guard.shutdown().await;
            return Err(e);
        }
        Err(_) => {
            guard.shutdown().await;
            return Err(anyhow!(
                "Timed out waiting for anvil to bind a port (forked RPC may be unreachable)"
            ));
        }
    };

    // Re-enable mining so eth_sendTransaction actually mines a block. We had
    // --no-mining only to silence the empty-block heartbeat.
    let local = format!("http://127.0.0.1:{port}");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;
    let _: Value = rpc(&client, &local, "evm_setAutomine", json!([true])).await?;

    // Step 3: replay the transaction.
    let result = run_simulation(&client, &local, &tx, block_number).await;
    guard.shutdown().await;
    result
}

async fn run_simulation(
    client: &reqwest::Client,
    local: &str,
    tx: &Value,
    block_number: u64,
) -> Result<SimulationResult> {
    let from = tx
        .get("from")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Original tx has no `from` field"))?
        .to_string();
    let to = tx.get("to").and_then(|v| v.as_str()).map(str::to_string);
    let value = tx
        .get("value")
        .and_then(|v| v.as_str())
        .unwrap_or("0x0")
        .to_string();
    let data = tx
        .get("input")
        .and_then(|v| v.as_str())
        .unwrap_or("0x")
        .to_string();
    let gas = tx
        .get("gas")
        .and_then(|v| v.as_str())
        .unwrap_or("0x100000")
        .to_string();

    // Top up the sender with absurdly large balance regardless of mainnet
    // value, because the replayed tx may itself transfer many ETH (whale
    // monitor!). Setting a uint128-max-ish value (~3.4×10^20 ETH) means the
    // simulation never spuriously reverts on insufficient-balance errors.
    let _: Value = rpc(
        client,
        local,
        "anvil_setBalance",
        json!([from, "0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF"]),
    )
    .await?;
    let _: Value = rpc(
        client,
        local,
        "anvil_impersonateAccount",
        json!([from]),
    )
    .await?;

    let mut tx_obj = serde_json::Map::new();
    tx_obj.insert("from".into(), json!(from));
    if let Some(to) = &to {
        tx_obj.insert("to".into(), json!(to));
    }
    tx_obj.insert("value".into(), json!(value));
    tx_obj.insert("data".into(), json!(data));
    tx_obj.insert("gas".into(), json!(gas));

    let send_resp = rpc(client, local, "eth_sendTransaction", json!([tx_obj])).await;
    let local_hash = match send_resp {
        Ok(v) => v.as_str().unwrap_or("").to_string(),
        Err(e) => {
            // anvil rejected the tx outright (e.g. insufficient gas, invalid
            // signature) — treat that as a revert with the rpc message.
            return Ok(SimulationResult {
                success: false,
                gas_used: 0,
                effective_gas_price_gwei: 0.0,
                revert_reason: Some(format!("anvil rejected tx: {e}")),
                transfers: vec![],
                log_count: 0,
                block_number,
            });
        }
    };

    let receipt: Value = rpc(
        client,
        local,
        "eth_getTransactionReceipt",
        json!([local_hash]),
    )
    .await?;

    if receipt.is_null() {
        return Err(anyhow!(
            "anvil produced no receipt for the simulated transaction"
        ));
    }

    let status_hex = receipt
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("0x0");
    let success = status_hex == "0x1";

    let gas_used = receipt
        .get("gasUsed")
        .and_then(|v| v.as_str())
        .and_then(parse_u64_hex)
        .unwrap_or(0);

    let gas_price_wei = receipt
        .get("effectiveGasPrice")
        .and_then(|v| v.as_str())
        .and_then(parse_u128_hex)
        .unwrap_or(0);
    let effective_gas_price_gwei = gas_price_wei as f64 / 1e9;

    let logs = receipt
        .get("logs")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let log_count = logs.len();

    let mut transfers = Vec::new();
    for log in &logs {
        if let Some(t) = decode_transfer(log) {
            transfers.push(t);
        }
    }

    let revert_reason = if !success {
        Some("Transaction reverted on the forked state".to_string())
    } else {
        None
    };

    Ok(SimulationResult {
        success,
        gas_used,
        effective_gas_price_gwei,
        revert_reason,
        transfers,
        log_count,
        block_number,
    })
}

/// `keccak256("Transfer(address,address,uint256)")`. ERC20, ERC721 and
/// ERC1155-via-wrapped contracts all share this topic for value transfers.
const TRANSFER_TOPIC: &str =
    "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";

fn decode_transfer(log: &Value) -> Option<DecodedTransfer> {
    let topics = log.get("topics")?.as_array()?;
    if topics.len() < 3 {
        return None;
    }
    let topic0 = topics[0].as_str()?;
    if !topic0.eq_ignore_ascii_case(TRANSFER_TOPIC) {
        return None;
    }
    let from = topic_to_address(topics[1].as_str()?);
    let to = topic_to_address(topics[2].as_str()?);
    let token = log.get("address")?.as_str()?.to_lowercase();
    let value_raw = log
        .get("data")
        .and_then(|v| v.as_str())
        .map(parse_data_uint256_decimal)
        .unwrap_or_else(|| "0".to_string());
    Some(DecodedTransfer {
        token,
        from,
        to,
        value_raw,
    })
}

fn topic_to_address(topic: &str) -> String {
    let s = topic.strip_prefix("0x").unwrap_or(topic);
    if s.len() < 40 {
        return format!("0x{}", s);
    }
    let addr = &s[s.len() - 40..];
    format!("0x{}", addr.to_lowercase())
}

fn parse_data_uint256_decimal(data: &str) -> String {
    let s = data.strip_prefix("0x").unwrap_or(data);
    // Take the last 64 hex chars (one 32-byte slot).
    let hex = if s.len() >= 64 {
        &s[s.len() - 64..]
    } else {
        s
    };
    let bytes = hex::decode(hex).unwrap_or_default();
    // Convert to decimal via successive base-256 pushes through u128, then
    // fall back to a textual big-int multiply for >128 bits.
    if bytes.len() <= 16 {
        let mut v: u128 = 0;
        for b in &bytes {
            v = v.wrapping_mul(256).wrapping_add(*b as u128);
        }
        return v.to_string();
    }
    big_decimal_from_be(&bytes)
}

fn big_decimal_from_be(bytes: &[u8]) -> String {
    // Decimal-string * 256 + b for each byte. Slow for 32 bytes (max ~78
    // decimal digits) but only runs on log decode hot paths, which max ~40
    // logs per simulation — negligible.
    let mut digits: Vec<u8> = vec![0];
    for &b in bytes {
        let mut carry: u32 = b as u32;
        for d in digits.iter_mut() {
            let v = (*d as u32) * 256 + carry;
            *d = (v % 10) as u8;
            carry = v / 10;
        }
        while carry > 0 {
            digits.push((carry % 10) as u8);
            carry /= 10;
        }
    }
    let mut s: String = digits.iter().rev().map(|d| (b'0' + d) as char).collect();
    if s.is_empty() {
        s.push('0');
    }
    s
}

fn parse_u64_hex(s: &str) -> Option<u64> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    u64::from_str_radix(s, 16).ok()
}

fn parse_u128_hex(s: &str) -> Option<u128> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    u128::from_str_radix(s, 16).ok()
}

async fn rpc(
    client: &reqwest::Client,
    url: &str,
    method: &str,
    params: Value,
) -> Result<Value> {
    let body = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
    let resp = client.post(url).json(&body).send().await?;
    let v: Value = resp.json().await?;
    if let Some(err) = v.get("error") {
        return Err(anyhow!("{}: {}", method, err));
    }
    Ok(v.get("result").cloned().unwrap_or(Value::Null))
}

async fn fetch_tx(http_url: &str, tx_hash: &str) -> Result<Value> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;
    let v = rpc(&client, http_url, "eth_getTransactionByHash", json!([tx_hash])).await?;
    if v.is_null() {
        return Err(anyhow!(
            "Upstream RPC returned null for the tx hash (it may have already been mined or replaced)"
        ));
    }
    Ok(v)
}

async fn fetch_block_number(http_url: &str) -> Result<u64> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;
    let v = rpc(&client, http_url, "eth_blockNumber", json!([])).await?;
    Ok(v.as_str().and_then(parse_u64_hex).unwrap_or(0))
}

async fn wait_for_port<R>(stdout: R) -> Result<u16>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut reader = BufReader::new(stdout).lines();
    while let Some(line) = reader.next_line().await? {
        if let Some(rest) = line.split_once("Listening on") {
            let part = rest.1.trim();
            // Format examples: "127.0.0.1:50321", "0.0.0.0:50321".
            if let Some(port_str) = part.rsplit(':').next() {
                if let Ok(p) = port_str.trim().parse::<u16>() {
                    return Ok(p);
                }
            }
        }
    }
    Err(anyhow!("anvil exited before binding a port"))
}
