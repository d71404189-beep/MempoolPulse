//! Gumroad license-key verification.

use serde::Deserialize;
use std::time::Duration;

const PRODUCT_PERMALINK: Option<&str> = option_env!("GUMROAD_PRODUCT_PERMALINK");

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

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build();
    let client = match client {
        Ok(c) => c,
        Err(e) => return (false, format!("HTTP client init failed: {}", e)),
    };

    let resp = client
        .post("https://api.gumroad.com/v2/licenses/verify")
        .form(&[
            ("product_permalink", permalink),
            ("license_key", trimmed),
            ("increment_uses_count", "false"),
        ])
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => return (false, format!("Network error contacting Gumroad: {}", e)),
    };

    let parsed: GumroadResponse = match resp.json().await {
        Ok(p) => p,
        Err(e) => return (false, format!("Could not parse Gumroad response: {}", e)),
    };

    if !parsed.success {
        return (
            false,
            parsed
                .message
                .unwrap_or_else(|| "License key not recognised.".into()),
        );
    }

    if let Some(p) = parsed.purchase {
        if p.refunded || p.chargebacked || p.disputed {
            return (false, "This license has been refunded or charged back.".into());
        }
    }

    (true, "License verified.".into())
}
