---
type: Playbook
title: Agent access through CDP
description: Enable a loopback CDP endpoint for browser agents and correlate targets with zz browser panes.
resource: crates/zz-browser/src/cef_runtime.rs
tags:
- browser
- cef
- cdp
- agents
timestamp: 2026-09-09T21:28:59Z
---

# Enable the endpoint

Set a bare integer in `zz/config`:

```ini
browser-remote-debugging-port = 9222
```

Leave the key unset or use `0` to disable CDP. Valid enabled ports are `1024..=65535`.
Invalid file values produce a config diagnostic and retain the preceding/default value.
Set `ZZ_BROWSER_REMOTE_DEBUGGING_PORT` in the client process environment to override the file.
An environment value of `0` disables CDP even when the file enables it; invalid values prevent
CEF initialization with a browser configuration error. The TUI uses the environment override.

Open a browser pane to initialize CEF and start listening. The GUI reads the file value before
that first initialization. After CEF starts, restart the client to change or disable the port.
There is no Settings UI control for this key.

CDP is off by default. Chromium listens on loopback (`127.0.0.1`); zz adds only the
`remote-debugging-port` switch and no `remote-debugging-address` switch. Any local process that
can reach this port can drive your logged-in browser, read pages, and run JavaScript with your
session's access. Enable it only while you intend to grant that access.

# Attach an agent

Replace `PORT` with the configured number:

```sh
agent-browser --cdp PORT snapshot -i
npx @playwright/mcp@latest --cdp-endpoint http://127.0.0.1:PORT
npx chrome-devtools-mcp@latest --browserUrl http://127.0.0.1:PORT
```

These recipes use the attach flags documented by [agent-browser](https://agent-browser.dev/cdp-mode),
[Playwright MCP](https://github.com/microsoft/playwright-mcp#configuration), and
[Chrome DevTools MCP](https://github.com/ChromeDevTools/chrome-devtools-mcp#connecting-to-a-running-chrome-instance).

# Match a pane to a CDP target

```sh
zz list-panes -a -F '#{pane_id} #{pane_kind} #{browser_url}'
zz display-message -p -t %0 '#{browser_url}'
curl http://127.0.0.1:PORT/json/list
```

Compare `browser_url` with each CDP target's `url`; the target list also provides titles.
`browser_url` contains the browser pane's active tab URL and an empty string for other pane kinds.
If several tabs share a URL, the URL alone cannot identify one target. CDP may also list background
tabs while the pane format reports only the active tab.

You can use the same variable in status formats and `refresh-client -B` subscriptions.
`MuxEngine::format_facts` in `crates/zz-mux/src/command.rs` reads `BrowserDescriptor::url()`;
`DaemonFormatHooks::variable` in `crates/zz-daemon/src/status.rs` exposes the value.

# Off-screen screenshots

For `Page.captureScreenshot`, set `clip.scale` to `1.0` when supplying a clip. CEF off-screen
rendering does not support custom capture scale; see [CEF issue 3103](https://github.com/chromiumembedded/cef/issues/3103)
and the [CefSharp off-screen capture implementation](https://github.com/cefsharp/CefSharp/blob/master/CefSharp.OffScreen/ChromiumWebBrowser.cs).
If an attached tool supplies another scale, adjust its screenshot request.

# Related

- [Application configuration](/configuration/app-config.md)
- [CEF runtime](/browser/cef-runtime.md)
- [Off-screen rendering](/browser/osr-rendering.md)
