import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, "..");

const EXTENSIONS = [
  {
    name: "chrome",
    globalKey: "chrome",
    source: "chrome",
    file: path.join(repoRoot, "chrome_extension", "background.js"),
    titleSuffix: " - Google Chrome",
  },
  {
    name: "firefox",
    globalKey: "browser",
    source: "firefox",
    file: path.join(repoRoot, "firefox_extension", "background.js"),
    titleSuffix: " — Mozilla Firefox",
  },
];

async function flushAsync() {
  for (let i = 0; i < 6; i += 1) {
    await Promise.resolve();
  }
  await new Promise((resolve) => setTimeout(resolve, 0));
}

function createHarness(ext, { windows, currentWindowId, lastFocusedWindowId }) {
  const state = {
    windows: structuredClone(windows),
    currentWindowId,
    lastFocusedWindowId,
  };

  const listeners = {
    tabActivated: [],
    tabUpdated: [],
    windowFocusChanged: [],
    alarm: [],
  };

  const sentMessages = [];
  const tabsQueryCalls = [];
  const windowGetCalls = [];
  const deferredWindowGets = new Map();

  const port = {
    onMessage: { addListener() {} },
    onDisconnect: { addListener() {} },
    postMessage(msg) {
      sentMessages.push(structuredClone(msg));
    },
  };

  const cloneWindow = (win, populate) => {
    const copy = {
      id: win.id,
      focused: win.focused,
    };
    if (populate) {
      copy.tabs = structuredClone(win.tabs);
    }
    return copy;
  };

  const api = {
    runtime: {
      lastError: null,
      connectNative() {
        return port;
      },
    },
    tabs: {
      async query(queryInfo) {
        tabsQueryCalls.push(structuredClone(queryInfo));
        const targetId = queryInfo.currentWindow
          ? state.currentWindowId
          : queryInfo.lastFocusedWindow
            ? state.lastFocusedWindowId
            : queryInfo.windowId;
        const win = state.windows.find((w) => w.id === targetId);
        if (!win) return [];
        return structuredClone(
          win.tabs.filter((t) => (queryInfo.active ? t.active : true)),
        );
      },
      async get(tabId) {
        for (const win of state.windows) {
          const tab = win.tabs.find((t) => t.id === tabId);
          if (tab) return structuredClone(tab);
        }
        throw new Error(`Unknown tab ${tabId}`);
      },
      onActivated: {
        addListener(fn) {
          listeners.tabActivated.push(fn);
        },
      },
      onUpdated: {
        addListener(fn) {
          listeners.tabUpdated.push(fn);
        },
      },
    },
    windows: {
      WINDOW_ID_NONE: -1,
      async get(windowId, options = {}) {
        windowGetCalls.push({ windowId, options: structuredClone(options) });
        if (deferredWindowGets.has(windowId)) {
          const queue = deferredWindowGets.get(windowId);
          if (queue.length > 0) {
            return queue.shift().promise;
          }
        }
        const win = state.windows.find((w) => w.id === windowId);
        if (!win) {
          throw new Error(`Unknown window ${windowId}`);
        }
        return cloneWindow(win, Boolean(options.populate));
      },
      async getLastFocused(options = {}) {
        const win =
          state.windows.find((w) => w.focused) ||
          state.windows.find((w) => w.id === state.lastFocusedWindowId);
        if (!win) {
          throw new Error("No windows available");
        }
        return cloneWindow(win, Boolean(options.populate));
      },
      onFocusChanged: {
        addListener(fn) {
          listeners.windowFocusChanged.push(fn);
        },
      },
    },
    alarms: {
      create() {},
      onAlarm: {
        addListener(fn) {
          listeners.alarm.push(fn);
        },
      },
    },
  };

  const sandbox = {
    URL,
    console: {
      log() {},
      warn() {},
      error() {},
    },
    [ext.globalKey]: api,
  };

  const code = fs.readFileSync(ext.file, "utf8");
  vm.createContext(sandbox);
  vm.runInContext(code, sandbox, { filename: ext.file });

  return {
    state,
    sentMessages,
    tabsQueryCalls,
    windowGetCalls,
    holdNextWindowGet(windowId) {
      let resolve;
      const promise = new Promise((r) => {
        resolve = r;
      });
      if (!deferredWindowGets.has(windowId)) {
        deferredWindowGets.set(windowId, []);
      }
      deferredWindowGets.get(windowId).push({ promise, resolve });
      return {
        resolve(winOverride) {
          const win =
            winOverride ?? state.windows.find((w) => w.id === windowId);
          resolve(cloneWindow(win, true));
        },
      };
    },
    async emitTabActivated(activeInfo) {
      for (const fn of listeners.tabActivated) {
        fn(activeInfo);
      }
      await flushAsync();
    },
    async emitTabUpdated(tabId, changeInfo, tab) {
      for (const fn of listeners.tabUpdated) {
        fn(tabId, changeInfo, tab);
      }
      await flushAsync();
    },
    async emitWindowFocusChanged(windowId) {
      for (const fn of listeners.windowFocusChanged) {
        fn(windowId);
      }
      await flushAsync();
    },
    async emitKeepaliveAlarm() {
      for (const fn of listeners.alarm) {
        fn({ name: "screentime-keepalive" });
      }
      await flushAsync();
    },
  };
}

for (const ext of EXTENSIONS) {
  test(`${ext.name}: startup only sends when a browser window is focused`, async () => {
    const unfocused = createHarness(ext, {
      currentWindowId: 1,
      lastFocusedWindowId: 1,
      windows: [
        {
          id: 1,
          focused: false,
          tabs: [
            {
              id: 10,
              windowId: 1,
              active: true,
              url: "https://example.com/bg",
              title: `Background${ext.titleSuffix}`,
            },
          ],
        },
      ],
    });
    await flushAsync();
    assert.equal(
      unfocused.sentMessages.length,
      0,
      "unfocused startup must not send stale background URL",
    );

    const focused = createHarness(ext, {
      currentWindowId: 1,
      lastFocusedWindowId: 1,
      windows: [
        {
          id: 1,
          focused: true,
          tabs: [
            {
              id: 10,
              windowId: 1,
              active: true,
              url: "https://www.rust-lang.org/",
              title: `Rust${ext.titleSuffix}`,
            },
          ],
        },
      ],
    });
    await flushAsync();
    assert.deepEqual(focused.sentMessages, [
      {
        type: "url_update",
        source: ext.source,
        url: "https://rust-lang.org/",
        title: "Rust",
      },
    ]);
  });

  test(`${ext.name}: ignores tabs.onUpdated and tabs.onActivated from unfocused windows`, async () => {
    const h = createHarness(ext, {
      currentWindowId: 2,
      lastFocusedWindowId: 2,
      windows: [
        {
          id: 1,
          focused: true,
          tabs: [
            {
              id: 11,
              windowId: 1,
              active: true,
              url: "https://github.com/rust-lang/rust",
              title: `Rust Repo${ext.titleSuffix}`,
            },
          ],
        },
        {
          id: 2,
          focused: false,
          tabs: [
            {
              id: 21,
              windowId: 2,
              active: false,
              url: "https://news.ycombinator.com/",
              title: `Hacker News${ext.titleSuffix}`,
            },
            {
              id: 22,
              windowId: 2,
              active: true,
              url: "https://docs.rs/",
              title: `Docs.rs${ext.titleSuffix}`,
            },
          ],
        },
      ],
    });
    await flushAsync();
    const initialCount = h.sentMessages.length;

    await h.emitTabUpdated(
      22,
      { title: `Updated Background${ext.titleSuffix}` },
      {
        id: 22,
        windowId: 2,
        active: true,
        url: "https://docs.rs/",
        title: `Updated Background${ext.titleSuffix}`,
      },
    );
    assert.equal(
      h.sentMessages.length,
      initialCount,
      "unfocused window tabs.onUpdated must be ignored",
    );

    h.state.windows[0].focused = false;
    h.state.windows[1].tabs[0].active = true;
    h.state.windows[1].tabs[1].active = false;
    await h.emitTabActivated({ tabId: 21, windowId: 2 });
    assert.equal(
      h.sentMessages.length,
      initialCount,
      "unfocused window tabs.onActivated must be ignored",
    );
  });

  test(`${ext.name}: windows.onFocusChanged queries exact windowId, forces resend on unchanged state, and drops stale async result on WINDOW_ID_NONE`, async () => {
    const h = createHarness(ext, {
      currentWindowId: 1,
      lastFocusedWindowId: 1,
      windows: [
        {
          id: 1,
          focused: true,
          tabs: [
            {
              id: 11,
              windowId: 1,
              active: true,
              url: "https://docs.rs/",
              title: `Docs.rs${ext.titleSuffix}`,
            },
          ],
        },
        {
          id: 2,
          focused: false,
          tabs: [
            {
              id: 21,
              windowId: 2,
              active: true,
              url: "https://www.youtube.com/watch?v=abc",
              title: `Async Rust - YouTube${ext.titleSuffix}`,
            },
          ],
        },
      ],
    });
    await flushAsync();
    h.sentMessages.length = 0;
    h.tabsQueryCalls.length = 0;
    h.windowGetCalls.length = 0;

    // Focus switches to window 2 while tabs.query(currentWindow) would still return window 1
    h.state.windows[0].focused = false;
    h.state.windows[1].focused = true;
    await h.emitWindowFocusChanged(2);

    assert.equal(
      h.tabsQueryCalls.some((q) => q.currentWindow === true),
      false,
      "windows.onFocusChanged must not rely on tabs.query({ currentWindow: true })",
    );
    assert.ok(
      h.windowGetCalls.some(
        (c) => c.windowId === 2 && c.options?.populate === true,
      ),
      "windows.onFocusChanged must call windows.get(windowId, { populate: true })",
    );
    assert.deepEqual(h.sentMessages, [
      {
        type: "url_update",
        source: ext.source,
        url: "https://youtube.com/watch?v=abc",
        title: "Async Rust - YouTube",
      },
    ]);

    // Refocusing window 2 with identical URL/title must force re-send
    h.sentMessages.length = 0;
    await h.emitWindowFocusChanged(2);
    assert.deepEqual(h.sentMessages, [
      {
        type: "url_update",
        source: ext.source,
        url: "https://youtube.com/watch?v=abc",
        title: "Async Rust - YouTube",
      },
    ]);

    // Interleaved WINDOW_ID_NONE must cancel in-flight windows.get(2)
    h.sentMessages.length = 0;
    const deferred = h.holdNextWindowGet(2);
    const focusPromise = h.emitWindowFocusChanged(2);
    h.state.windows[1].focused = false;
    await h.emitWindowFocusChanged(-1);
    deferred.resolve({
      id: 2,
      focused: true,
      tabs: h.state.windows[1].tabs,
    });
    await focusPromise;

    assert.deepEqual(
      h.sentMessages,
      [
        {
          type: "url_update",
          source: ext.source,
          url: null,
          title: null,
        },
      ],
      "stale windows.get resolution after WINDOW_ID_NONE must be dropped",
    );
  });

  test(`${ext.name}: keepalive alarm re-sends when focused and stays silent when unfocused`, async () => {
    const h = createHarness(ext, {
      currentWindowId: 1,
      lastFocusedWindowId: 1,
      windows: [
        {
          id: 1,
          focused: true,
          tabs: [
            {
              id: 11,
              windowId: 1,
              active: true,
              url: "https://crates.io/",
              title: `crates.io${ext.titleSuffix}`,
            },
          ],
        },
      ],
    });
    await flushAsync();
    assert.equal(h.sentMessages.length, 1);

    await h.emitKeepaliveAlarm();
    assert.equal(
      h.sentMessages.length,
      2,
      "keepalive alarm must force re-send when window is focused",
    );

    h.state.windows[0].focused = false;
    await h.emitWindowFocusChanged(-1);
    h.sentMessages.length = 0;

    await h.emitKeepaliveAlarm();
    assert.equal(
      h.sentMessages.length,
      0,
      "keepalive alarm must stay silent when unfocused",
    );
  });

  test(`${ext.name}: handles changeInfo.status === "complete" and falls back to url on whitespace-only title`, async () => {
    const h = createHarness(ext, {
      currentWindowId: 1,
      lastFocusedWindowId: 1,
      windows: [
        {
          id: 1,
          focused: true,
          tabs: [
            {
              id: 11,
              windowId: 1,
              active: true,
              url: "https://www.example.com/start",
              title: `Start${ext.titleSuffix}`,
            },
          ],
        },
      ],
    });
    await flushAsync();
    h.sentMessages.length = 0;

    await h.emitTabUpdated(
      11,
      { status: "complete" },
      {
        id: 11,
        windowId: 1,
        active: true,
        url: "https://www.example.com/loaded",
        title: `Loaded Page${ext.titleSuffix}`,
      },
    );
    assert.deepEqual(h.sentMessages, [
      {
        type: "url_update",
        source: ext.source,
        url: "https://example.com/loaded",
        title: "Loaded Page",
      },
    ]);

    h.sentMessages.length = 0;
    await h.emitTabUpdated(
      11,
      { title: `   ${ext.titleSuffix}` },
      {
        id: 11,
        windowId: 1,
        active: true,
        url: "https://www.example.com/empty-title",
        title: `   ${ext.titleSuffix}`,
      },
    );
    assert.deepEqual(h.sentMessages, [
      {
        type: "url_update",
        source: ext.source,
        url: "https://example.com/empty-title",
        title: "https://example.com/empty-title",
      },
    ]);
  });
}
