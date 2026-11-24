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
      type: win.type ?? "normal",
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
        const allowedTypes =
          options?.windowTypes ??
          (ext.name === "chrome" ? ["normal", "popup"] : null);
        const candidates = state.windows.filter((w) => {
          if (!allowedTypes) return true;
          return allowedTypes.includes(w.type ?? "normal");
        });
        const win =
          candidates.find((w) => w.focused) ||
          candidates.find((w) => w.id === state.lastFocusedWindowId);
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

    h.state.windows[0].tabs[0] = {
      id: 11,
      windowId: 1,
      active: true,
      url: "https://www.example.com/loaded",
      title: `Loaded Page${ext.titleSuffix}`,
    };
    await h.emitTabUpdated(
      11,
      { status: "complete" },
      structuredClone(h.state.windows[0].tabs[0]),
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
    h.state.windows[0].tabs[0] = {
      id: 11,
      windowId: 1,
      active: true,
      url: "https://www.example.com/empty-title",
      title: `   ${ext.titleSuffix}`,
    };
    await h.emitTabUpdated(
      11,
      { title: `   ${ext.titleSuffix}` },
      structuredClone(h.state.windows[0].tabs[0]),
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

  test(`${ext.name}: tabs.onUpdated racing with tabs.onActivated or live tab changes never reverts to stale snapshot`, async () => {
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
              url: "https://example.com/one",
              title: `One${ext.titleSuffix}`,
            },
            {
              id: 12,
              windowId: 1,
              active: false,
              url: "https://example.com/two",
              title: `Two${ext.titleSuffix}`,
            },
          ],
        },
      ],
    });
    await flushAsync();
    h.sentMessages.length = 0;

    const deferredUpdate = h.holdNextWindowGet(1);
    const updatePromise = h.emitTabUpdated(
      11,
      { title: `Stale One${ext.titleSuffix}` },
      {
        id: 11,
        windowId: 1,
        active: true,
        url: "https://example.com/one",
        title: `Stale One${ext.titleSuffix}`,
      },
    );

    h.state.windows[0].tabs[0].active = false;
    h.state.windows[0].tabs[1].active = true;
    await h.emitTabActivated({ tabId: 12, windowId: 1 });

    deferredUpdate.resolve({
      id: 1,
      type: "normal",
      focused: true,
      tabs: [
        {
          id: 11,
          windowId: 1,
          active: true,
          url: "https://example.com/one",
          title: `Stale One${ext.titleSuffix}`,
        },
      ],
    });
    await updatePromise;

    assert.deepEqual(
      h.sentMessages,
      [
        {
          type: "url_update",
          source: ext.source,
          url: "https://example.com/two",
          title: "Two",
        },
      ],
      "stale tabs.onUpdated completion after tabs.onActivated must be dropped",
    );

    h.sentMessages.length = 0;
    const deferredLive = h.holdNextWindowGet(1);
    const livePromise = h.emitTabUpdated(
      12,
      { title: `Pre-Await Two${ext.titleSuffix}` },
      {
        id: 12,
        windowId: 1,
        active: true,
        url: "https://example.com/two",
        title: `Pre-Await Two${ext.titleSuffix}`,
      },
    );
    h.state.windows[0].tabs[1].title = `Live Two${ext.titleSuffix}`;
    deferredLive.resolve();
    await livePromise;

    assert.deepEqual(
      h.sentMessages,
      [
        {
          type: "url_update",
          source: ext.source,
          url: "https://example.com/two",
          title: "Live Two",
        },
      ],
      "tabs.onUpdated must publish live active tab state rather than pre-await snapshot",
    );
  });

  test(`${ext.name}: unfocused background window sync resolving mid-flight does not cancel focused sync or drop pending force`, async () => {
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
              url: "https://example.com/one",
              title: `One${ext.titleSuffix}`,
            },
            {
              id: 12,
              windowId: 1,
              active: false,
              url: "https://example.com/focused-next",
              title: `Focused Next${ext.titleSuffix}`,
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
              url: "https://example.com/bg-one",
              title: `Background One${ext.titleSuffix}`,
            },
            {
              id: 22,
              windowId: 2,
              active: false,
              url: "https://example.com/bg-two",
              title: `Background Two${ext.titleSuffix}`,
            },
          ],
        },
      ],
    });
    await flushAsync();
    h.sentMessages.length = 0;

    const deferredActivated = h.holdNextWindowGet(1);
    h.state.windows[0].tabs[0].active = false;
    h.state.windows[0].tabs[1].active = true;
    const focusedPromise = h.emitTabActivated({ tabId: 12, windowId: 1 });

    await h.emitTabUpdated(
      21,
      { title: `Background Updated${ext.titleSuffix}` },
      {
        id: 21,
        windowId: 2,
        active: true,
        url: "https://example.com/bg-one",
        title: `Background Updated${ext.titleSuffix}`,
      },
    );
    h.state.windows[1].tabs[0].active = false;
    h.state.windows[1].tabs[1].active = true;
    await h.emitTabActivated({ tabId: 22, windowId: 2 });

    deferredActivated.resolve();
    await focusedPromise;
    await flushAsync();

    assert.deepEqual(
      h.sentMessages,
      [
        {
          type: "url_update",
          source: ext.source,
          url: "https://example.com/focused-next",
          title: "Focused Next",
        },
      ],
      "unfocused background window events must not cancel in-flight focused window sync",
    );

    h.sentMessages.length = 0;
    const deferredForced = h.holdNextWindowGet(1);
    const forcedPromise = h.emitWindowFocusChanged(1);

    await h.emitTabUpdated(
      22,
      { title: `Background Two Updated${ext.titleSuffix}` },
      {
        id: 22,
        windowId: 2,
        active: true,
        url: "https://example.com/bg-two",
        title: `Background Two Updated${ext.titleSuffix}`,
      },
    );

    deferredForced.resolve();
    await forcedPromise;
    await flushAsync();

    assert.deepEqual(
      h.sentMessages,
      [
        {
          type: "url_update",
          source: ext.source,
          url: "https://example.com/focused-next",
          title: "Focused Next",
        },
      ],
      "unfocused background window sync must not clear pending force for in-flight focused sync",
    );
  });

  test(`${ext.name}: overlapping non-forced tabs.onUpdated preserves force from pending windows.onFocusChanged`, async () => {
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
              url: "https://example.com/same",
              title: `Same${ext.titleSuffix}`,
            },
          ],
        },
      ],
    });
    await flushAsync();
    h.sentMessages.length = 0;

    const deferredFocus = h.holdNextWindowGet(1);
    const focusPromise = h.emitWindowFocusChanged(1);

    await h.emitTabUpdated(
      11,
      { status: "complete" },
      structuredClone(h.state.windows[0].tabs[0]),
    );

    deferredFocus.resolve();
    await focusPromise;
    await flushAsync();

    assert.deepEqual(
      h.sentMessages,
      [
        {
          type: "url_update",
          source: ext.source,
          url: "https://example.com/same",
          title: "Same",
        },
      ],
      "overlapping non-forced tabs.onUpdated must preserve force: true and send once even when url/title match prior state",
    );
  });
}

test("chrome: startup and keepalive alarm find focused type 'app' PWA window when normal window is unfocused", async () => {
  const chromeExt = EXTENSIONS[0];
  const h = createHarness(chromeExt, {
    currentWindowId: 1,
    lastFocusedWindowId: 1,
    windows: [
      {
        id: 1,
        type: "normal",
        focused: false,
        tabs: [
          {
            id: 11,
            windowId: 1,
            active: true,
            url: "https://github.com/rust-lang/rust",
            title: "Rust Repo - Google Chrome",
          },
        ],
      },
      {
        id: 2,
        type: "app",
        focused: true,
        tabs: [
          {
            id: 21,
            windowId: 2,
            active: true,
            url: "https://www.youtube.com/watch?v=pwa123",
            title: "PWA Video - YouTube - Google Chrome",
          },
        ],
      },
    ],
  });
  await flushAsync();

  assert.deepEqual(
    h.sentMessages,
    [
      {
        type: "url_update",
        source: "chrome",
        url: "https://youtube.com/watch?v=pwa123",
        title: "PWA Video - YouTube",
      },
    ],
    "Chrome startup must include 'app' windowTypes to find focused PWA window",
  );

  h.sentMessages.length = 0;
  await h.emitKeepaliveAlarm();
  assert.deepEqual(
    h.sentMessages,
    [
      {
        type: "url_update",
        source: "chrome",
        url: "https://youtube.com/watch?v=pwa123",
        title: "PWA Video - YouTube",
      },
    ],
    "Chrome keepalive alarm must include 'app' windowTypes to find focused PWA window",
  );
});
