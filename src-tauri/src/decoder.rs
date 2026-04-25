//! Lightweight, allocation-friendly decoder for the most common Ethereum mainnet
//! method calls we care about in the mempool: Uniswap V2/V3 router swaps, the
//! Universal Router, and ERC20 `transfer` / `approve`.
//!
//! We deliberately hand-roll selector matching here instead of pulling in a full
//! ABI engine — it keeps cold compile times short and the binary small. Each
//! decoder returns `(label, summary)` where `label` is a short canonical name
//! and `summary` is a one-line plaintext description suitable for the UI.

/// Match a 4-byte selector to a known method and return `(label, summary_hint)`.
///
/// `input_hex` is the full input data including the leading `0x`. We parse only
/// what we need from the encoded args; any malformed payload falls back to the
/// label without a detailed summary.
pub fn decode(input_hex: &str) -> (Option<String>, Option<String>) {
    if input_hex.len() < 10 {
        return (None, None);
    }
    let selector = &input_hex[..10].to_lowercase();
    let body = &input_hex[10..];

    match selector.as_str() {
        // Uniswap V2 Router02
        "0x7ff36ab5" => (
            Some("Uniswap V2: swapExactETHForTokens".into()),
            decode_v2_eth_for_tokens(body),
        ),
        "0xfb3bdb41" => (
            Some("Uniswap V2: swapETHForExactTokens".into()),
            None,
        ),
        "0x18cbafe5" => (
            Some("Uniswap V2: swapExactTokensForETH".into()),
            decode_v2_tokens_for_eth(body),
        ),
        "0x4a25d94a" => (
            Some("Uniswap V2: swapTokensForExactETH".into()),
            None,
        ),
        "0x38ed1739" => (
            Some("Uniswap V2: swapExactTokensForTokens".into()),
            decode_v2_tokens_for_tokens(body),
        ),
        "0x8803dbee" => (
            Some("Uniswap V2: swapTokensForExactTokens".into()),
            None,
        ),
        "0xf305d719" => (
            Some("Uniswap V2: addLiquidityETH".into()),
            None,
        ),
        "0xe8e33700" => (
            Some("Uniswap V2: addLiquidity".into()),
            None,
        ),

        // Uniswap V3 SwapRouter / SwapRouter02
        "0x414bf389" => (
            Some("Uniswap V3: exactInputSingle".into()),
            None,
        ),
        "0xc04b8d59" => (
            Some("Uniswap V3: exactInput".into()),
            None,
        ),
        "0xdb3e2198" => (
            Some("Uniswap V3: exactOutputSingle".into()),
            None,
        ),
        "0xf28c0498" => (
            Some("Uniswap V3: exactOutput".into()),
            None,
        ),

        // Universal Router (Uniswap)
        "0x3593564c" => (Some("Uniswap Universal Router: execute".into()), None),
        "0x24856bc3" => (Some("Uniswap Universal Router: execute (deadline)".into()), None),

        // ERC20
        "0xa9059cbb" => (
            Some("ERC20: transfer".into()),
            decode_erc20_transfer(body),
        ),
        "0x23b872dd" => (
            Some("ERC20: transferFrom".into()),
            None,
        ),
        "0x095ea7b3" => (
            Some("ERC20: approve".into()),
            decode_erc20_approve(body),
        ),

        // Common WETH
        "0xd0e30db0" => (Some("WETH: deposit".into()), None),
        "0x2e1a7d4d" => (Some("WETH: withdraw".into()), None),

        // Multicall
        "0xac9650d8" => (Some("Multicall".into()), None),

        // Safe / Account abstraction common
        "0x6a761202" => (Some("Gnosis Safe: execTransaction".into()), None),

        _ => (None, None),
    }
}

/// Read a 32-byte slot at a given slot index from an ABI-encoded body (no `0x`).
fn slot(body: &str, idx: usize) -> Option<&str> {
    let start = idx * 64;
    let end = start + 64;
    if end > body.len() {
        None
    } else {
        Some(&body[start..end])
    }
}

/// Parse a 32-byte ABI slot as a uint256, returning a lossy f64 (good enough for UI).
fn slot_to_f64(slot_hex: &str) -> Option<f64> {
    // We only need approximate magnitudes. Strip leading zeros and parse the
    // remaining hex into a u128 if it fits; otherwise saturate.
    let trimmed = slot_hex.trim_start_matches('0');
    if trimmed.is_empty() {
        return Some(0.0);
    }
    if trimmed.len() <= 32 {
        u128::from_str_radix(trimmed, 16).ok().map(|n| n as f64)
    } else {
        // Too large for u128; approximate via top 32 hex chars * 16^(remaining).
        let top = &trimmed[..32];
        let rest = trimmed.len() - 32;
        u128::from_str_radix(top, 16)
            .ok()
            .map(|n| (n as f64) * 16f64.powi(rest as i32))
    }
}

/// Last 20 bytes of a 32-byte slot interpreted as an Ethereum address.
fn slot_to_addr(slot_hex: &str) -> Option<String> {
    if slot_hex.len() != 64 {
        return None;
    }
    Some(format!("0x{}", &slot_hex[24..]))
}

fn decode_v2_eth_for_tokens(body: &str) -> Option<String> {
    // swapExactETHForTokens(uint256 amountOutMin, address[] path, address to, uint256 deadline)
    let _amount_out_min = slot(body, 0)?;
    // path is dynamic; offset is in slot 1 but we don't need full decoding for UI.
    Some("Swap ETH → tokens via V2 router".into())
}

fn decode_v2_tokens_for_eth(body: &str) -> Option<String> {
    // swapExactTokensForETH(uint256 amountIn, uint256 amountOutMin, address[] path, address to, uint256 deadline)
    let amount_in = slot_to_f64(slot(body, 0)?)?;
    Some(format!(
        "Swap tokens → ETH via V2 (amountIn ≈ {})",
        format_compact(amount_in)
    ))
}

fn decode_v2_tokens_for_tokens(body: &str) -> Option<String> {
    let amount_in = slot_to_f64(slot(body, 0)?)?;
    Some(format!(
        "Swap tokens → tokens via V2 (amountIn ≈ {})",
        format_compact(amount_in)
    ))
}

fn decode_erc20_transfer(body: &str) -> Option<String> {
    // transfer(address to, uint256 amount)
    let to = slot_to_addr(slot(body, 0)?)?;
    let amount = slot_to_f64(slot(body, 1)?)?;
    Some(format!(
        "Transfer ≈ {} (raw) → {}",
        format_compact(amount),
        short_addr(&to)
    ))
}

fn decode_erc20_approve(body: &str) -> Option<String> {
    let spender = slot_to_addr(slot(body, 0)?)?;
    let amount = slot_to_f64(slot(body, 1)?)?;
    let amt_str = if amount > 1e30 {
        "unlimited".into()
    } else {
        format_compact(amount)
    };
    Some(format!(
        "Approve {} for spender {}",
        amt_str,
        short_addr(&spender)
    ))
}

fn short_addr(addr: &str) -> String {
    if addr.len() < 10 {
        return addr.to_string();
    }
    format!("{}…{}", &addr[..6], &addr[addr.len() - 4..])
}

fn format_compact(n: f64) -> String {
    if n >= 1e18 {
        format!("{:.2}e18", n / 1e18)
    } else if n >= 1e15 {
        format!("{:.2}e15", n / 1e15)
    } else if n >= 1e9 {
        format!("{:.2}B", n / 1e9)
    } else if n >= 1e6 {
        format!("{:.2}M", n / 1e6)
    } else if n >= 1e3 {
        format!("{:.2}K", n / 1e3)
    } else {
        format!("{:.0}", n)
    }
}

/// Convert a 0x-prefixed hex `value` (as returned by JSON-RPC) into a wei `f64`.
/// Suitable for ETH-magnitude values; loses precision on extreme uints.
pub fn parse_hex_wei(hex_str: &str) -> Option<f64> {
    let s = hex_str.strip_prefix("0x").unwrap_or(hex_str);
    if s.is_empty() {
        return Some(0.0);
    }
    // Trim leading zeros to fit in u128 when possible.
    let trimmed = s.trim_start_matches('0');
    if trimmed.is_empty() {
        return Some(0.0);
    }
    if trimmed.len() <= 32 {
        u128::from_str_radix(trimmed, 16).ok().map(|v| v as f64)
    } else {
        let top = &trimmed[..32];
        let rest = trimmed.len() - 32;
        u128::from_str_radix(top, 16)
            .ok()
            .map(|n| (n as f64) * 16f64.powi(rest as i32))
    }
}

/// Convert a 0x-prefixed hex value to a decimal string (preserving full precision).
pub fn hex_to_decimal_string(hex_str: &str) -> String {
    let s = hex_str.strip_prefix("0x").unwrap_or(hex_str);
    let trimmed = s.trim_start_matches('0');
    if trimmed.is_empty() {
        return "0".into();
    }
    // Big-int free decimal: use repeated u128 chunks.
    // For UI this is fine; we won't do math on the string.
    if let Ok(n) = u128::from_str_radix(trimmed, 16) {
        n.to_string()
    } else {
        // Fall back to hex display for absurdly large numbers.
        format!("0x{}", trimmed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_lookup_matches_known_methods() {
        let (label, _) = decode("0xa9059cbb00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000001");
        assert!(label.unwrap().contains("transfer"));
        let (label, _) = decode("0x38ed1739");
        assert_eq!(label, Some("Uniswap V2: swapExactTokensForTokens".into()));
    }

    #[test]
    fn parse_hex_wei_handles_basic_values() {
        assert_eq!(parse_hex_wei("0x0"), Some(0.0));
        assert_eq!(parse_hex_wei("0xde0b6b3a7640000"), Some(1e18));
    }

    #[test]
    fn unknown_selector_returns_none() {
        let (label, summary) = decode("0xdeadbeef");
        assert!(label.is_none());
        assert!(summary.is_none());
    }
}
