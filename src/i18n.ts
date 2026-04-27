export type Lang = "en" | "ru";
export type LangPref = Lang | "auto";

const messages = {
  en: {
    "app.loading": "Loading…",
    "app.tab.live": "Live feed",
    "app.tab.settings": "Settings",
    "app.stats.txs": "txs",

    "license.title": "MempoolPulse",
    "license.intro": "Enter your license key to unlock the live mempool feed.",
    "license.purchase_prefix": "You can purchase a key from",
    "license.placeholder": "XXXX-XXXX-XXXX-XXXX",
    "license.activate": "Activate",
    "license.verifying": "Verifying…",

    "live.empty.title": "Waiting for pending transactions…",
    "live.empty.body_prefix": "Make sure your WebSocket RPC URL is set in Settings and that your provider supports the",
    "live.empty.body_or": "or",
    "live.empty.body_suffix": "subscription.",
    "live.search_placeholder": "Filter by hash, address, or method…",
    "live.matched": "matched",
    "live.export.csv": "Export CSV",
    "live.export.json": "Export JSON",
    "live.export.tooltip": "Export the current filtered rows to a file",
    "live.col.chain": "Chain",
    "live.col.hash": "Hash",
    "live.col.from": "From",
    "live.col.to": "To",
    "live.col.value_eth": "Value (ETH)",
    "live.col.value": "Value",
    "live.col.usd": "USD",
    "live.col.gas": "Gas (gwei)",
    "live.col.method": "Method",
    "live.create": "create",
    "live.raw_call": "raw call",

    "settings.title": "Settings",
    "settings.lead": "Bring your own RPC. MempoolPulse never proxies your traffic — your keys and pending-tx data stay on this machine.",
    "settings.section.rpc": "RPC endpoints",
    "settings.field.ws": "WebSocket URL",
    "settings.field.https": "HTTPS URL (optional, used to enrich hash-only feeds)",
    "settings.section.chains": "Chains",
    "settings.chains.lead": "Enable a chain to start streaming. Each chain uses its own RPC; the WebSocket URL is required, HTTPS is optional but recommended for hash-only providers.",
    "settings.chain.enabled": "Enabled",
    "settings.chain.name": "Name",
    "settings.chain.symbol": "Native",
    "settings.chain.ws": "WebSocket URL",
    "settings.chain.https": "HTTPS URL (optional)",
    "settings.chains.reset": "Reset to defaults",
    "settings.section.filters": "Filters",
    "settings.field.min_eth": "Min value (ETH)",
    "settings.field.min_usd": "Min value (USD)",
    "settings.field.buffer": "Buffer size",
    "settings.field.contracts": "Filter to contracts (one address per line)",
    "settings.field.selectors": "Function selectors (4-byte hex, one per line)",
    "settings.section.watchlist": "Watchlist",
    "settings.watch.address": "0xWalletAddress",
    "settings.watch.label": "Label (e.g. Whale #1)",
    "settings.watch.add": "+ Add address",
    "settings.watch.remove": "Remove",
    "settings.section.lang": "Language",
    "settings.lang.label": "Interface language",
    "settings.lang.auto": "Auto (system)",
    "settings.lang.en": "English",
    "settings.lang.ru": "Русский",
    "settings.save": "Save",
    "settings.saving": "Saving…",
    "settings.saved_at": "Saved",

    "status.idle": "Idle",
    "status.reconnect": "Reconnect",
    "status.stop": "Stop",
    "status.no_rpc": "No RPC WebSocket URL configured.",
    "status.connecting": "Connecting to {url}",
    "status.streaming": "Streaming pending transactions",
    "status.closed": "Connection closed by server.",
    "status.disconnected": "Disconnected: {err}. Retrying in {secs}s.",
    "status.stopped": "Stopped",
    "status.aggregate": "Streaming on {connected}/{total} chains",
    "status.aggregate_one": "{detail}",
    "status.aggregate_none": "All chains stopped",
    "status.no_chains": "No chains enabled. Open Settings to enable one.",
  },
  ru: {
    "app.loading": "Загрузка…",
    "app.tab.live": "Живой поток",
    "app.tab.settings": "Настройки",
    "app.stats.txs": "трз",

    "license.title": "MempoolPulse",
    "license.intro": "Введите лицензионный ключ, чтобы открыть живой поток mempool.",
    "license.purchase_prefix": "Купить ключ можно на",
    "license.placeholder": "XXXX-XXXX-XXXX-XXXX",
    "license.activate": "Активировать",
    "license.verifying": "Проверка…",

    "live.empty.title": "Ожидание pending-транзакций…",
    "live.empty.body_prefix": "Убедитесь, что в Настройках указан WebSocket RPC URL и что ваш провайдер поддерживает подписку",
    "live.empty.body_or": "или",
    "live.empty.body_suffix": ".",
    "live.search_placeholder": "Поиск по hash, адресу или методу…",
    "live.matched": "найдено",
    "live.export.csv": "Экспорт в CSV",
    "live.export.json": "Экспорт в JSON",
    "live.export.tooltip": "Экспортировать текущие отфильтрованные строки в файл",
    "live.col.chain": "Сеть",
    "live.col.hash": "Hash",
    "live.col.from": "Откуда",
    "live.col.to": "Куда",
    "live.col.value_eth": "Сумма (ETH)",
    "live.col.value": "Сумма",
    "live.col.usd": "USD",
    "live.col.gas": "Gas (gwei)",
    "live.col.method": "Метод",
    "live.create": "создание",
    "live.raw_call": "raw-вызов",

    "settings.title": "Настройки",
    "settings.lead": "Используйте свой RPC. MempoolPulse не проксирует трафик — ключи и данные mempool остаются на вашей машине.",
    "settings.section.rpc": "RPC-эндпоинты",
    "settings.field.ws": "WebSocket URL",
    "settings.field.https": "HTTPS URL (опционально, для обогащения hash-only потоков)",
    "settings.section.chains": "Сети",
    "settings.chains.lead": "Включите сеть, чтобы начать стрим. У каждой сети свой RPC; WebSocket URL обязателен, HTTPS — опциональный, нужен для hash-only провайдеров.",
    "settings.chain.enabled": "Включена",
    "settings.chain.name": "Название",
    "settings.chain.symbol": "Монета",
    "settings.chain.ws": "WebSocket URL",
    "settings.chain.https": "HTTPS URL (опционально)",
    "settings.chains.reset": "Сбросить к значениям по умолчанию",
    "settings.section.filters": "Фильтры",
    "settings.field.min_eth": "Мин. сумма (ETH)",
    "settings.field.min_usd": "Мин. сумма (USD)",
    "settings.field.buffer": "Размер буфера",
    "settings.field.contracts": "Только контракты (по одному адресу на строку)",
    "settings.field.selectors": "Function-селекторы (4 байта hex, по одному на строку)",
    "settings.section.watchlist": "Watchlist",
    "settings.watch.address": "0xАдресКошелька",
    "settings.watch.label": "Метка (например, Кит №1)",
    "settings.watch.add": "+ Добавить адрес",
    "settings.watch.remove": "Удалить",
    "settings.section.lang": "Язык",
    "settings.lang.label": "Язык интерфейса",
    "settings.lang.auto": "Авто (система)",
    "settings.lang.en": "English",
    "settings.lang.ru": "Русский",
    "settings.save": "Сохранить",
    "settings.saving": "Сохранение…",
    "settings.saved_at": "Сохранено",

    "status.idle": "Простой",
    "status.reconnect": "Переподключить",
    "status.stop": "Стоп",
    "status.no_rpc": "RPC WebSocket URL не указан.",
    "status.connecting": "Подключение к {url}",
    "status.streaming": "Поток pending-транзакций",
    "status.closed": "Соединение закрыто сервером.",
    "status.disconnected": "Отключено: {err}. Повтор через {secs}с.",
    "status.stopped": "Остановлено",
    "status.aggregate": "Стрим в {connected}/{total} сетях",
    "status.aggregate_one": "{detail}",
    "status.aggregate_none": "Все сети остановлены",
    "status.no_chains": "Ни одна сеть не включена. Откройте Настройки.",
  },
} as const;

export type MessageKey = keyof typeof messages.en;

export function detectLanguage(): Lang {
  if (typeof navigator === "undefined") return "en";
  const raw = (navigator.language || "en").toLowerCase();
  if (raw.startsWith("ru") || raw.startsWith("be") || raw.startsWith("kk") || raw.startsWith("uk")) {
    return "ru";
  }
  return "en";
}

export function resolveLang(pref: LangPref | undefined | null): Lang {
  if (pref === "en" || pref === "ru") return pref;
  return detectLanguage();
}

export function t(lang: Lang, key: MessageKey): string {
  return messages[lang][key] ?? messages.en[key] ?? key;
}

/** Render a localized status template, substituting {placeholders}. */
export function formatStatus(
  lang: Lang,
  status: { code?: string | null; params?: Record<string, string> | null; message?: string },
): string {
  const code = status.code as MessageKey | undefined;
  if (!code) return status.message ?? "";
  const tpl = (messages[lang] as Record<string, string>)[code]
    ?? (messages.en as Record<string, string>)[code]
    ?? status.message
    ?? code;
  const params = status.params ?? {};
  return tpl.replace(/\{(\w+)\}/g, (_, k) => params[k] ?? `{${k}}`);
}
