/**
 * ScreenTime Chrome Extension — Background Service Worker
 *
 * Tracks the active tab URL and sends domain + title to the
 * native messaging host, which writes it to a state file
 * for the daemon to read.
 */

const NATIVE_HOST_NAME = "com.screentime.native";
const ALARM_NAME = "screentime-keepalive";
const ALARM_INTERVAL_MINUTES = 1.0; // ~60 seconds

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
    port = chrome.runtime.connectNative(NATIVE_HOST_NAME);

    port.onMessage.addListener((msg) => {
      // Response from native host — currently just status acks
      // console.log("Native host response:", msg);
    });

    port.onDisconnect.addListener(() => {
      const error = chrome.runtime.lastError;
      if (error) {
        console.warn("Native host disconnected:", error.message);
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
        source: "chrome",
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
    // Skip internal Chrome pages
    if (
      url.startsWith("chrome://") ||
      url.startsWith("chrome-extension://") ||
      url.startsWith("about:") ||
      url.startsWith("edge://")
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
    .replace(/\s*[-–—]\s*Google Chrome\s*$/i, "")
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
        ? await chrome.windows.get(windowId, { populate: true })
        : await chrome.windows.getLastFocused({
            populate: true,
            windowTypes: ["normal", "popup", "app"],
          });
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

chrome.tabs.onActivated.addListener((activeInfo) => {
  syncFocusedWindow({ windowId: activeInfo?.windowId ?? null });
});

chrome.tabs.onUpdated.addListener((_tabId, changeInfo, tab) => {
  if (
    (!changeInfo.url && !changeInfo.title && changeInfo.status !== "complete") ||
    !tab?.active ||
    tab.windowId == null
  ) {
    return;
  }
  syncFocusedWindow({ windowId: tab.windowId });
});

chrome.windows.onFocusChanged.addListener((windowId) => {
  if (windowId !== chrome.windows.WINDOW_ID_NONE) {
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

// Set up a periodic alarm to keep the service worker alive
// and re-send the current URL
chrome.alarms.create(ALARM_NAME, {
  periodInMinutes: ALARM_INTERVAL_MINUTES,
});

chrome.alarms.onAlarm.addListener((alarm) => {
  if (alarm.name === ALARM_NAME) {
    syncFocusedWindow({ force: true });
  }
});

// ── Startup ──────────────────────────────────────────────────────────

// Connect to native host and send initial URL on service worker start
connectNativeHost();
syncFocusedWindow();

console.log("ScreenTime extension background service worker started");
