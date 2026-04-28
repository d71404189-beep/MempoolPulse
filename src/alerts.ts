//! Frontend-side alert engine: programmatic Web Audio sounds + rule matching.
//!
//! Sounds are synthesised on the fly with a single `AudioContext` so we don't
//! have to ship audio asset files or worry about licensing. Each sound is a
//! short envelope of one or two oscillators tuned to be distinguishable but
//! not annoying.

import type { AlertRule, AlertSound, AppSettings, PendingTx } from "./types";

let ctx: AudioContext | null = null;
function getCtx(): AudioContext {
  if (!ctx) ctx = new AudioContext();
  if (ctx.state === "suspended") void ctx.resume();
  return ctx;
}

interface Tone {
  freq: number;
  startAt: number; // seconds offset from now
  duration: number; // seconds
  type?: OscillatorType;
  peakGain?: number; // 0..1
}

const SOUND_RECIPES: Record<AlertSound, Tone[]> = {
  // Single short blip — minimal, good for low-importance triggers.
  ping: [{ freq: 880, startAt: 0, duration: 0.08, type: "sine", peakGain: 0.15 }],
  // Three-note arpeggio — pleasant default for most rules.
  chime: [
    { freq: 660, startAt: 0, duration: 0.12, type: "sine", peakGain: 0.16 },
    { freq: 880, startAt: 0.1, duration: 0.12, type: "sine", peakGain: 0.16 },
    { freq: 1320, startAt: 0.2, duration: 0.18, type: "sine", peakGain: 0.16 },
  ],
  // Decaying bell tone — clearly draws attention without being shrill.
  bell: [
    { freq: 440, startAt: 0, duration: 0.6, type: "triangle", peakGain: 0.22 },
    { freq: 880, startAt: 0, duration: 0.6, type: "sine", peakGain: 0.08 },
  ],
  // Two-tone alternation — for "this is big" alerts (>$1M whale, etc).
  siren: [
    { freq: 700, startAt: 0, duration: 0.18, type: "sawtooth", peakGain: 0.18 },
    { freq: 1000, startAt: 0.18, duration: 0.18, type: "sawtooth", peakGain: 0.18 },
    { freq: 700, startAt: 0.36, duration: 0.18, type: "sawtooth", peakGain: 0.18 },
    { freq: 1000, startAt: 0.54, duration: 0.18, type: "sawtooth", peakGain: 0.18 },
  ],
};

/**
 * Plays a sound. Safe to call on hot paths; if the AudioContext fails (e.g.
 * before any user interaction in the webview) the call simply no-ops.
 */
export function playSound(id: AlertSound): void {
  try {
    const audio = getCtx();
    const recipe = SOUND_RECIPES[id] ?? SOUND_RECIPES.chime;
    const now = audio.currentTime;
    for (const tone of recipe) {
      const osc = audio.createOscillator();
      const gain = audio.createGain();
      osc.type = tone.type ?? "sine";
      osc.frequency.value = tone.freq;
      const peak = tone.peakGain ?? 0.18;
      const start = now + tone.startAt;
      const end = start + tone.duration;
      // Quick attack + exponential-ish decay shaped with two linear ramps so
      // the click on stop is gentle.
      gain.gain.setValueAtTime(0.0001, start);
      gain.gain.linearRampToValueAtTime(peak, start + Math.min(0.015, tone.duration * 0.3));
      gain.gain.exponentialRampToValueAtTime(0.0001, end);
      osc.connect(gain);
      gain.connect(audio.destination);
      osc.start(start);
      osc.stop(end + 0.02);
    }
  } catch {
    // ignore — autoplay blocked or AudioContext unavailable
  }
}

export const SOUND_OPTIONS: AlertSound[] = ["ping", "chime", "bell", "siren"];

/**
 * Build a stable random id for a new rule.
 */
export function newRuleId(): string {
  return `rule-${Math.random().toString(36).slice(2, 10)}`;
}

/**
 * Three default rules users can opt into via "Load preset rules".
 */
export function presetRules(): AlertRule[] {
  return [
    {
      id: newRuleId(),
      name: "Whale swap > $500k",
      enabled: true,
      chains: [],
      min_value_usd: 500_000,
      label_contains: "swap",
      watchlist_only: false,
      sound: "siren",
      cooldown_secs: 30,
    },
    {
      id: newRuleId(),
      name: "Watchlist hit",
      enabled: true,
      chains: [],
      min_value_usd: null,
      label_contains: null,
      watchlist_only: true,
      sound: "chime",
      cooldown_secs: 5,
    },
    {
      id: newRuleId(),
      name: "Mega transfer > $1M",
      enabled: true,
      chains: [],
      min_value_usd: 1_000_000,
      label_contains: null,
      watchlist_only: false,
      sound: "bell",
      cooldown_secs: 60,
    },
  ];
}

/**
 * Returns the rule that matches a tx, or null. The first matching rule wins
 * (so users can put more specific rules higher in the list if they care).
 */
export function matchRule(
  tx: PendingTx,
  settings: AppSettings,
): AlertRule | null {
  const watch = new Set(
    settings.watchlist.map((w) => w.address.toLowerCase()).filter(Boolean),
  );
  for (const rule of settings.alert_rules) {
    if (!rule.enabled) continue;
    if (rule.chains.length > 0 && !rule.chains.includes(tx.chain)) continue;
    if (rule.min_value_usd != null && (tx.value_usd ?? 0) < rule.min_value_usd) continue;
    if (rule.label_contains && rule.label_contains.trim()) {
      const needle = rule.label_contains.trim().toLowerCase();
      const hay = (tx.label ?? "").toLowerCase();
      if (!hay.includes(needle)) continue;
    }
    if (rule.watchlist_only) {
      const fromHit = watch.has(tx.from.toLowerCase());
      const toHit = tx.to ? watch.has(tx.to.toLowerCase()) : false;
      if (!fromHit && !toHit) continue;
    }
    return rule;
  }
  return null;
}

/**
 * Renders a short toast description for a rule match.
 */
export function describeMatch(tx: PendingTx, rule: AlertRule): string {
  const usd = tx.value_usd != null ? ` $${Math.round(tx.value_usd).toLocaleString("en-US")}` : "";
  const label = tx.label ? ` · ${tx.label}` : "";
  return `${rule.name}:${usd}${label}`;
}
