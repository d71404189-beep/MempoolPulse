//! Gumroad license-key verification.

use serde::Deserialize;
use std::time::Duration;

const PRODUCT_PERMALINK: Option<&str> = option_env!("GUMROAD_PRODUCT_PERMALINK");
const PRODUCT_ID: Option<&str> = option_env!("GUMROAD_PRODUCT_ID");

#[derive(Deserialize)]
struct GumroadResponse {
    success: bool,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    purchase: Option<GumroadPurchase>,
}

#[derive(Deserialize)]
struct GumroadPurchase {
    #[serde(default)]
    refunded: bool,
    #[serde(default)]
    chargebacked: bool,
    #[serde(default)]
    disputed: bool,
}

pub async fn verify(key: &str) -> (bool, String) {
    let trimmed = key.trim();
    if trimmed.is_empty() {
        return (false, "License key is empty.".into());
    }

    let permalink = match PRODUCT_PERMALINK {
        Some(p) if !p.is_empty() => p,
        _ => {
            return (true, "Development mode (no product permalink set).".into());
        }
    };

    let client = match reqwest::Client::builder().timeout(Duration::from_secs(15)).build() {
        Ok(c) => c,
        Err(e) => return (false, format!("HTTP client error: {}", e)),
    };

    // Build params - always send product_permalink
    // Also send product_id if available (some Gumroad products require it)
    let mut params: Vec<(&str, &str)> = vec![
        ("product_permalink", permalink),
        ("license_key", trimmed),
        ("increment_uses_count", "false"),
    ];

    let pid = PRODUCT_ID.unwrap_or("").trim();
    if !pid.is_empty() {
        params.push(("product_id", pid));
    }

    let resp = match client
        .post("https://api.gumroad.com/v2/licenses/verify")
        .form(&params)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return (false, format!("Network error: {}", e)),
    };

    let parsed: GumroadResponse = match resp.json().await {
        Ok(p) => p,
        Err(e) => return (false, format!("Parse error: {}", e)),
    };

    if !parsed.success {
        return (
            false,
            parsed.message.unwrap_or_else(|| "License key not recognised.".into()),
        );
    }

    if let Some(p) = parsed.purchase {
        if p.refunded || p.chargebacked || p.disputed {
            return (false, "This license has been refunded or charged back.".into());
        }
    }

    (true, "License verified.".into())
}
