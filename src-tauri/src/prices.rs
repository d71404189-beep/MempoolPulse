//! Price fetching for native ETH (and a handful of common stables).
//!
//! Uses the public CoinGecko endpoint which has a generous free tier and does
//! not require an API key for low-volume use. Results are cached for 60s to
//! stay well within rate limits.

use parking_lot::RwLock;
use serde::Deserialize;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
pub struct PriceCache {
    pub eth_usd: f64,
    pub fetched_at: Instant,
}

impl Default for PriceCache {
    fn default() -> Self {
        Self {
            eth_usd: 0.0,
            // Force a refresh on first call.
            fetched_at: Instant::now() - Duration::from_secs(3600),
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct PriceFetcher {
    inner: Arc<RwLock<PriceCache>>,
}

impl PriceFetcher {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the cached ETH/USD price. Refreshes from CoinGecko if older than 60s.
    /// Falls back to whatever is currently cached on network errors so the UI never
    /// shows blank values just because CoinGecko is briefly unavailable.
    pub async fn eth_usd(&self) -> f64 {
        {
            let cache = self.inner.read();
            if cache.fetched_at.elapsed() < Duration::from_secs(60) && cache.eth_usd > 0.0 {
                return cache.eth_usd;
            }
        }

        if let Ok(price) = fetch_eth_usd().await {
            let mut cache = self.inner.write();
            cache.eth_usd = price;
            cache.fetched_at = Instant::now();
            return price;
        }

        // Network failure: return last known value (may be 0 on cold start).
        self.inner.read().eth_usd
    }
}

#[derive(Deserialize)]
struct CoinGeckoResp {
    ethereum: CoinGeckoUsd,
}

#[derive(Deserialize)]
struct CoinGeckoUsd {
    usd: f64,
}

async fn fetch_eth_usd() -> anyhow::Result<f64> {
    let resp: CoinGeckoResp = reqwest::Client::new()
        .get("https://api.coingecko.com/api/v3/simple/price?ids=ethereum&vs_currencies=usd")
        .timeout(Duration::from_secs(8))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(resp.ethereum.usd)
}
