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
let currentDomain = null;
let currentTitle = null;

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
  // Remove common suffixes like " — Mozilla Firefox"
  return tabTitle
    .replace(/\s*[-–—]\s*Mozilla Firefox\s*$/i, "")
    .replace(/\s*[-–—]\s*Firefox\s*$/i, "")
    .trim();
}

// ── Tab Tracking ─────────────────────────────────────────────────────

async function updateCurrentTab() {
  try {
    const tabs = await browser.tabs.query({
      active: true,
      currentWindow: true,
    });

    const tab = tabs[0];
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
browser.tabs.onActivated.addListener((_activeInfo) => {
  updateCurrentTab();
});

// Tab updated (page loaded, URL changed, title changed)
browser.tabs.onUpdated.addListener((tabId, changeInfo, tab) => {
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
browser.windows.onFocusChanged.addListener((windowId) => {
  if (windowId !== browser.windows.WINDOW_ID_NONE) {
    updateCurrentTab();
  }
});

// ── Keep-alive via Alarms ────────────────────────────────────────────

browser.alarms.create(ALARM_NAME, {
  periodInMinutes: ALARM_INTERVAL_MINUTES,
});

browser.alarms.onAlarm.addListener((alarm) => {
  if (alarm.name === ALARM_NAME) {
    updateCurrentTab();
  }
});

// ── Startup ──────────────────────────────────────────────────────────

connectNativeHost();
updateCurrentTab();

console.log("ScreenTime extension background script started");
