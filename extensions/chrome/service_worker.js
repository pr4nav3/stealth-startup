const BRIDGE_PORTS = Array.from({ length: 11 }, (_, index) => 43177 + index);
const BRIDGE_HEADER = "x-supervisor-browser-bridge";
const BRIDGE_HEADER_VALUE = "1";
const FETCH_TIMEOUT_MS = 700;

chrome.runtime.onInstalled.addListener(() => {
  void relayActiveTab();
});

chrome.runtime.onStartup.addListener(() => {
  void relayActiveTab();
});

chrome.tabs.onActivated.addListener(() => {
  void relayActiveTab();
});

chrome.tabs.onUpdated.addListener((_tabId, changeInfo, tab) => {
  if (!tab.active) {
    return;
  }
  if (changeInfo.url || changeInfo.title || changeInfo.status === "complete") {
    void relayActiveTab();
  }
});

chrome.windows.onFocusChanged.addListener((windowId) => {
  if (windowId !== chrome.windows.WINDOW_ID_NONE) {
    void relayActiveTab();
  }
});

async function relayActiveTab() {
  const [tab] = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
  const payload = payloadForTab(tab);
  if (!payload) {
    return;
  }

  for (const port of BRIDGE_PORTS) {
    try {
      const response = await fetchWithTimeout(`http://127.0.0.1:${port}/tab`, {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          "X-Supervisor-Browser-Bridge": BRIDGE_HEADER_VALUE,
        },
        body: JSON.stringify(payload),
        cache: "no-store",
      });
      if (response.status === 204 && response.headers.get(BRIDGE_HEADER) === BRIDGE_HEADER_VALUE) {
        return;
      }
    } catch {
      // Supervisor may not be running yet.
    }
  }
}

async function fetchWithTimeout(url, options) {
  const controller = new AbortController();
  const timeoutId = setTimeout(() => controller.abort(), FETCH_TIMEOUT_MS);
  try {
    return await fetch(url, { ...options, signal: controller.signal });
  } finally {
    clearTimeout(timeoutId);
  }
}

function payloadForTab(tab) {
  if (typeof tab?.id !== "number" || !tab.url) {
    return null;
  }

  return {
    browser: browserName(),
    tab_id: tab.id,
    window_id: tab.windowId ?? -1,
    url: tab.url,
    title: tab.title || null,
    domain: domainForUrl(tab.url),
    captured_at: new Date().toISOString(),
  };
}

function browserName() {
  return "chrome";
}

function domainForUrl(url) {
  try {
    const parsed = new URL(url);
    return parsed.hostname || null;
  } catch {
    return null;
  }
}
