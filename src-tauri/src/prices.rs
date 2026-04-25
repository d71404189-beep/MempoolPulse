//! Price fetching for native chain assets (ETH, BNB, …).
//!
//! Uses the public CoinGecko endpoint which has a generous free tier and does
//! not require an API key for low-volume use. Results are cached for 60s per
//! coin id to stay well within rate limits.

use parking_lot::RwLock;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
struct CachedPrice {
    usd: f64,
    fetched_at: Instant,
}

#[derive(Default, Clone)]
pub struct PriceFetcher {
    inner: Arc<RwLock<HashMap<String, CachedPrice>>>,
}

impl PriceFetcher {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the cached USD price for a CoinGecko coin id (e.g. "ethereum",
    /// "binancecoin"). Refreshes from CoinGecko if older than 60s. Falls back
    /// to whatever is currently cached on network errors so the UI never
    /// shows blank values just because CoinGecko is briefly unavailable.
    pub async fn usd(&self, coingecko_id: &str) -> f64 {
        if coingecko_id.is_empty() {
            return 0.0;
        }
        {
            let cache = self.inner.read();
            if let Some(c) = cache.get(coingecko_id) {
                if c.fetched_at.elapsed() < Duration::from_secs(60) && c.usd > 0.0 {
                    return c.usd;
                }
            }
        }
        if let Ok(price) = fetch_usd(coingecko_id).await {
            self.inner.write().insert(
                coingecko_id.to_string(),
                CachedPrice {
                    usd: price,
                    fetched_at: Instant::now(),
                },
            );
            return price;
        }
        // Network failure: return last known value (may be 0 on cold start).
        self.inner
            .read()
            .get(coingecko_id)
            .map(|c| c.usd)
            .unwrap_or(0.0)
    }
}

#[derive(Deserialize)]
struct CoinGeckoUsd {
    usd: f64,
}

async fn fetch_usd(coingecko_id: &str) -> anyhow::Result<f64> {
    let url = format!(
        "https://api.coingecko.com/api/v3/simple/price?ids={}&vs_currencies=usd",
        coingecko_id
    );
    let resp: HashMap<String, CoinGeckoUsd> = reqwest::Client::new()
        .get(&url)
        .timeout(Duration::from_secs(8))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(resp.get(coingecko_id).map(|c| c.usd).unwrap_or(0.0))
}
