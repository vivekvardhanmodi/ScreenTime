/**
 * ScreenTime Firefox Extension — Background Script
 *
 * Tracks the active tab URL and sends domain + title to the
 * native messaging host, which writes it to a state file
 * for the daemon to read.
 *
 * Firefox uses `browser.*` APIs (WebExtension standard) and
 * persistent background pages instead of service workers.
 */

const NATIVE_HOST_NAME = "com.screentime.native";
const ALARM_NAME = "screentime-keepalive";
const ALARM_INTERVAL_MINUTES = 0.4; // ~25 seconds

let port = null;
let currentUrl = null;
let currentTitle = null;
let nextSeq = 0;
let lastAppliedSeq = 0;
let pendingForceSeq = 0;

// ── Native Messaging Connection ──────────────────────────────────────

function connectNativeHost() {
  if (port) {
    return;
  }

  try {
    port = browser.runtime.connectNative(NATIVE_HOST_NAME);

    port.onMessage.addListener((msg) => {
      // Response from native host — currently just status acks
    });

    port.onDisconnect.addListener((p) => {
      if (p.error) {
        console.warn("Native host disconnected:", p.error.message);
      }
      port = null;
      // Will reconnect on next URL update
    });

    console.log("Connected to native messaging host");
  } catch (e) {
    console.error("Failed to connect to native host:", e);
    port = null;
  }
}

function sendUrlUpdate(url, title) {
  if (!port) {
    connectNativeHost();
  }

  if (port) {
    try {
      port.postMessage({
        type: "url_update",
        source: "firefox",
        url: url,
        title: title,
      });
    } catch (e) {
      console.error("Failed to send URL update:", e);
      port = null;
    }
  }
}

// ── URL Extraction ───────────────────────────────────────────────────

function extractUrl(url) {
  if (!url) return null;

  try {
    // Skip internal browser pages
    if (
      url.startsWith("about:") ||
      url.startsWith("moz-extension://") ||
      url.startsWith("chrome://") ||
      url.startsWith("resource://")
    ) {
      return null;
    }

    const parsed = new URL(url);
    
    // Remove 'www.' prefix for cleaner tracking
    if (parsed.hostname.startsWith("www.")) {
      parsed.hostname = parsed.hostname.substring(4);
    }

    return parsed.href || null;
  } catch {
    return null;
  }
}

function extractTitle(tabTitle, url) {
  if (!tabTitle) return url;
  const cleaned = tabTitle
    .replace(/\s*[-–—]\s*Mozilla Firefox\s*$/i, "")
    .replace(/\s*[-–—]\s*Firefox\s*$/i, "")
    .trim();
  return cleaned || url;
}

// ── Tab Tracking ─────────────────────────────────────────────────────

function publishTab(tab, { force = false } = {}) {
  if (!tab || !tab.url) {
    return;
  }

  const url = extractUrl(tab.url);
  const title = extractTitle(tab.title, url);

  if (force || url !== currentUrl || title !== currentTitle) {
    currentUrl = url;
    currentTitle = title;
    sendUrlUpdate(url, title);
  }
}

async function syncFocusedWindow({ windowId = null, force = false } = {}) {
  const seq = ++nextSeq;
  if (force) {
    pendingForceSeq = seq;
  }
  try {
    const win =
      windowId != null
        ? await browser.windows.get(windowId, { populate: true })
        : await browser.windows.getLastFocused({ populate: true });
    if (seq <= lastAppliedSeq) {
      return;
    }
    if (!win || !win.focused) {
      if (seq === pendingForceSeq) {
        pendingForceSeq = 0;
      }
      return;
    }
    lastAppliedSeq = seq;
    const shouldForce =
      force || (pendingForceSeq > 0 && pendingForceSeq <= seq);
    if (pendingForceSeq <= seq) {
      pendingForceSeq = 0;
    }
    const tab = win.tabs?.find((t) => t.active);
    publishTab(tab, { force: shouldForce });
  } catch (e) {
    if (seq === pendingForceSeq) {
      pendingForceSeq = 0;
    }
    console.error("Failed to sync focused window:", e);
  }
}

browser.tabs.onActivated.addListener((activeInfo) => {
  syncFocusedWindow({ windowId: activeInfo?.windowId ?? null });
});

browser.tabs.onUpdated.addListener((_tabId, changeInfo, tab) => {
  if (
    (!changeInfo.url && !changeInfo.title && changeInfo.status !== "complete") ||
    !tab?.active ||
    tab.windowId == null
  ) {
    return;
  }
  syncFocusedWindow({ windowId: tab.windowId });
});

browser.windows.onFocusChanged.addListener((windowId) => {
  if (windowId !== browser.windows.WINDOW_ID_NONE) {
    syncFocusedWindow({ windowId, force: true });
  } else {
    lastAppliedSeq = ++nextSeq;
    pendingForceSeq = 0;
    currentUrl = null;
    currentTitle = null;
    sendUrlUpdate(null, null);
  }
});

// ── Keep-alive via Alarms ────────────────────────────────────────────

browser.alarms.create(ALARM_NAME, {
  periodInMinutes: ALARM_INTERVAL_MINUTES,
});

browser.alarms.onAlarm.addListener((alarm) => {
  if (alarm.name === ALARM_NAME) {
    syncFocusedWindow({ force: true });
  }
});

// ── Startup ──────────────────────────────────────────────────────────

connectNativeHost();
syncFocusedWindow();

console.log("ScreenTime extension background script started");
