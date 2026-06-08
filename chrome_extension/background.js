/**
 * ScreenTime Chrome Extension — Background Service Worker
 *
 * Tracks the active tab URL and sends domain + title to the
 * native messaging host, which writes it to a state file
 * for the daemon to read.
 */

const NATIVE_HOST_NAME = "com.screentime.native";
const ALARM_NAME = "screentime-keepalive";
const ALARM_INTERVAL_MINUTES = 0.4; // ~25 seconds

let port = null;
let currentDomain = null;
let currentTitle = null;

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

function sendUrlUpdate(domain, title) {
  if (!port) {
    connectNativeHost();
  }

  if (port) {
    try {
      port.postMessage({
        type: "url_update",
        domain: domain,
        title: title,
      });
    } catch (e) {
      console.error("Failed to send URL update:", e);
      port = null;
    }
  }
}

// ── URL Extraction ───────────────────────────────────────────────────

function extractDomain(url) {
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
    let hostname = parsed.hostname;

    // Remove 'www.' prefix for cleaner tracking
    if (hostname.startsWith("www.")) {
      hostname = hostname.substring(4);
    }

    return hostname || null;
  } catch {
    return null;
  }
}

function extractTitle(tabTitle, domain) {
  if (!tabTitle) return domain;
  // Remove common suffixes like " - Google Chrome"
  return tabTitle
    .replace(/\s*[-–—]\s*Google Chrome\s*$/i, "")
    .trim();
}

// ── Tab Tracking ─────────────────────────────────────────────────────

async function updateCurrentTab() {
  try {
    const [tab] = await chrome.tabs.query({
      active: true,
      currentWindow: true,
    });

    if (!tab || !tab.url) {
      return;
    }

    const domain = extractDomain(tab.url);
    const title = extractTitle(tab.title, domain);

    // Only send update if domain changed
    if (domain !== currentDomain || title !== currentTitle) {
      currentDomain = domain;
      currentTitle = title;
      sendUrlUpdate(domain, title);
    }
  } catch (e) {
    console.error("Failed to query active tab:", e);
  }
}

// Tab activated (user switched tabs)
chrome.tabs.onActivated.addListener((_activeInfo) => {
  updateCurrentTab();
});

// Tab updated (page loaded, URL changed, title changed)
chrome.tabs.onUpdated.addListener((tabId, changeInfo, tab) => {
  if (changeInfo.url || changeInfo.title) {
    // Only process if this is the active tab
    if (tab.active) {
      const domain = extractDomain(tab.url);
      const title = extractTitle(tab.title, domain);

      if (domain !== currentDomain || title !== currentTitle) {
        currentDomain = domain;
        currentTitle = title;
        sendUrlUpdate(domain, title);
      }
    }
  }
});

// Window focus changed
chrome.windows.onFocusChanged.addListener((windowId) => {
  if (windowId !== chrome.windows.WINDOW_ID_NONE) {
    updateCurrentTab();
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
    updateCurrentTab();
  }
});

// ── Startup ──────────────────────────────────────────────────────────

// Connect to native host and send initial URL on service worker start
connectNativeHost();
updateCurrentTab();

console.log("ScreenTime extension background service worker started");
