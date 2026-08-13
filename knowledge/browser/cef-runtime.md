---
type: Subsystem
title: CEF runtime & subprocess dispatch
description: CEF Alloy OSR bootstrap, single-binary subprocess dispatch, frame-rate policy, external BeginFrames, message pumping, and safe foreground command dispatch.
resource: crates/zz-browser/src/cef_runtime.rs
tags: [browser, cef, runtime, subprocess, begin-frame, frame-pacing]
timestamp: 2026-08-12T00:00:00Z
---

# Overview

The CEF runtime subsystem is `cef_runtime.rs` plus the crate root `lib.rs`. It owns
the one-time global CEF Alloy initialization, the **single-binary multi-process
model** (one executable acts as the browser process and every Chromium
subprocess), and the **external message pump** that steps CEF from GPUI's
foreground executor instead of a CEF-owned loop. CEF is pinned to Rust package
`151.2.0+151.3.14`, backed by **Chromium `151.0.7922.72`**; upgrading is an
explicit dependency bump requiring all platform bundle smoke tests
(see [updating CEF](/playbooks/updating-cef.md)).

CEF binding types never cross the crate boundary. The runtime translates CEF
callbacks into browser-neutral [`BrowserEvent`](/browser/lifecycle.md) values and
publishes owned frames into a [mailbox](/browser/osr-rendering.md).

# Single-binary multi-process model

zz ships one executable that Chromium re-executes for its zygote, GPU, renderer,
and utility subprocesses. `bootstrap()` decides which role the current process
plays by calling CEF `execute_process` early:

- `execute_process` returns `>= 0` → this is a **subprocess**; return
  `BrowserBootstrap::SubprocessExit(code)` and never start GPUI.
- returns exactly `-1` → this is the **main browser process**; resolve the
  profile, initialize CEF, and return `BrowserBootstrap::Runtime(BrowserRuntime)`.
- any other negative value → `BrowserError::ExecuteProcess`.

`run_subprocess()` is the dedicated helper entry point (used by the macOS helper
app, which initializes a `cef::sandbox::Sandbox` and loads the framework via
`LibraryLoader`). On Windows, `bootstrap_windows(instance, sandbox_info)` routes
through CEF's sandbox bootstrap executable and the exported `RunWinMain` entry.

# Initialization settings

`bootstrap_args` builds the global CEF `Settings`:

| Setting | Value | Purpose |
| --- | --- | --- |
| `windowless_rendering_enabled` | `1` | Enable OSR (no native child window). |
| `external_message_pump` | `1` | zz drives the loop; CEF requests wakes via callback. |
| `no_sandbox` | `0` | Sandbox stays **enabled**; no `--no-sandbox` fallback exists. |
| `root_cache_path` | profile `root` | Parent of every persistent zz profile (see [profile](/browser/profile.md)). |
| `persist_session_cookies` | `1` | Keep session cookies across restarts. |
| `log_severity` | `WARNING` | Quiet CEF's own logging. |
| `background_color` | `0xff101318` | Opaque dark backdrop matching the app theme. |

`on_before_command_line_processing` appends switches to every process
(`no-first-run`, `no-default-browser-check`, `disable-component-update`,
`disable-breakpad`, `hide-crash-restore-bubble`). The browser process also gets
`disable-features=ImmersiveReadAnything` (merged into any existing value; children
inherit feature state): Chromium 151 enables Immersive Reading Mode by default and
its soft-navigation observer segfaults on windowless WebContents, which carry no
tab user-data . any SPA route change would kill the browser process (see the
2026-08-03 [update log](/log.md) entry). Chromium's GPU process and
shared-texture OSR are enabled by default. `ZZ_BROWSER_SHARED_TEXTURE=0` keeps
the GPU process but selects `on_paint` readback, while `ZZ_BROWSER_GPU=0` adds
`disable-gpu` + `disable-gpu-compositing` to the **browser process only** and forces
the software readback path. The runtime retains an atomic readback fallback if GPU
texture import fails. The handler also adds `use-mock-keychain` to macOS debug
browser processes so ad-hoc rebuilds do not repeatedly prompt for Chromium Safe
Storage, and on Linux adds
`ozone-platform-hint=auto` and `disable-setuid-sandbox`. The
legacy setuid sandbox is disabled while Chromium's user-namespace sandbox stays on,
because dev bundles cannot carry the root-owned setuid helper bit.

Chromium 151 treats the `auto` hint as native Wayland when the session reports
Wayland, so zz does not default to XWayland on NVIDIA. The `ozone-platform=x11`
and `use-angle=gl-egl` pair belongs to CEF's cefclient OSR sample, not libcef's
automatic platform selection. Forcing that pair did not initialize a browser in
zz's Wayland-hosted fixture, so the application keeps Ozone selection automatic.

# Runtime environment

| Variable | Default | Effect |
| --- | --- | --- |
| `ZZ_BROWSER_FPS=1..240` | Unset | Explicit per-session OSR ceiling. On macOS, an unset value derives the ceiling from the fastest attached `NSScreen`; other platforms retain 60 FPS. Invalid explicit values fall back to 60. |
| `ZZ_BROWSER_GPU=0` | GPU enabled | Disables Chromium GPU rendering/compositing and therefore shared-texture OSR. |
| `ZZ_BROWSER_SHARED_TEXTURE=0` | Shared textures enabled | Keeps Chromium GPU content acceleration but selects the universal `on_paint` readback tier. On Linux, each visible shared-texture session gets a two-second first-frame guard. Any delivered frame cancels the guard, and hiding the pane pauses it. If the guard expires, zz recreates only that session in readback mode while keeping the GPU process enabled. This covers failures before CEF emits either paint callback. On the tested NVIDIA host, CEF 151's default GL path hit that failure; native Vulkan produced frames but failed resize stress under Chromium's unsupported Ozone/Wayland combination. See the [NVIDIA accelerated OSR investigation](/research/2026-08-07-nvidia-cef-accelerated-osr.md). |
| `ZZ_BROWSER_EXTERNAL_BEGIN_FRAME` | macOS on; Linux/FreeBSD off | On macOS, exact `0` restores CEF's internal BeginFrame timer. On Linux/FreeBSD, exact `1` opts into zz-driven BeginFrames; all other values leave them off. |
| `ZZ_BROWSER_BF_ADAPTIVE=1` | Adaptive throttle disabled | Opts into delivery-based BeginFrame divisor tiers on the anchored clock; all other values leave adaptation off. |

The effective ceiling is computed once at controller initialization. Focused
sessions use it. Visible unfocused sessions use at most 30 FPS, except that wheel
input temporarily restores the ceiling and re-arms a one-second decay. Hidden
sessions remain stopped through `was_hidden`.

# External message pump

CEF never runs its own loop here. Instead:

1. `RuntimeBrowserProcessHandler::on_schedule_message_pump_work(delay_ms)` fires a
   `RuntimeSignal::ScheduleMessagePump(delay_ms)` over an `async-channel`.
2. The app arms a reschedulable GPUI timer and clones the runtime's
   `BrowserMessagePump` handle at the deadline.
3. The GPUI entity update returns, releasing the app's `RefCell` borrow, before
   `BrowserMessagePump::do_message_loop_work()` enters CEF on the main thread.
4. The handle calls `cef::do_message_loop_work()` only while initialized and not
   in a `Closed`/`Failed` phase; nested pump attempts coalesce into a follow-up
   iteration.

Releasing GPUI state before entering CEF is a required re-entrancy boundary, not
an ownership convenience. CEF may synchronously route a macOS keyboard event
through `NSMenu`, whose validation/action callbacks borrow the GPUI app again.
Pumping CEF from inside `Entity::update` therefore turns an ordinary menu shortcut
into a double-borrow panic, which aborts because the callback cannot unwind across
Objective-C. The normal timer and shutdown loop both use the detached pump handle.

Browser commands use the same boundary. `BrowserController` captures an opaque,
main-thread-only `BrowserCommandSink` from the target session and stores owned
keyboard, text, IME, edit, navigation, history, and reload operations in one FIFO.
The first operation schedules a foreground task; that task takes the whole burst
inside a short controller update, lets the update return, then invokes each CEF
operation in order. Taking the queue and calling it are deliberately separate:
putting the CEF call inside `Entity::update`, `App::defer`, or another effect flush
would restore the app double borrow. Capturing the sink also prevents delayed input
from being looked up against a replacement tab or session.

The app applies bounded fallbacks around CEF-requested deadlines. Runtime
initialization, browser creation/closing, cookie import, and current-origin data
clear use a 16ms interval while their callbacks are outstanding. A visible ready
browser uses a 33ms maximum wait, matching cefclient's 30Hz external-pump
watchdog; CEF can still replace it with an earlier requested deadline. This
prevents a visible interactive page from stalling when Chromium has not yet had
a time slice to discover new work, without restoring a continuous pump for
hidden idle panes. A hidden-to-visible transition requests an immediate turn.

Accelerated OSR frames only reach zz during `do_message_loop_work` turns and CEF
does not request per-frame wakes, so pump cadence is the effective frame-rate
cap. While a visible session delivered a frame or received input within the
last 500ms (`PUMP_HOT_WINDOW`), the fallback tightens to the frame interval for
the effective ceiling (240 FPS → ~4.2ms, never slower than the watchdog), and
the cold→hot edge requests an immediate turn; the cadence decays back to the
30Hz watchdog once activity stops. Input stamping matters for ramp-up: a scroll
on a cold static page would otherwise wait two watchdog laps (BeginFrame out,
frame back) before the pump tightened.

When external BeginFrames are enabled, every pump request is also clamped to the
earliest per-pane compositor deadline. After each turn the controller arms the
minimum of that deadline and the existing fallback interval; later CEF wake
requests therefore cannot postpone a due compositor tick. Existing immediate
zero-delay kicks remain intact.

`BrowserRuntime` and its sessions share an active-data-operation counter;
shutdown waits for both browser sessions and those operations, preventing a
cookie-store flush from being cut off when the last pane closes.

`RuntimeSignal` also carries lifecycle handshakes: `ContextInitialized` (from
`on_context_initialized`) and `RequestContextInitialized { profile }` (from each
request-context handler). The app creates `default` after global initialization;
its tagged callback advances the runtime to `Running`. A pane requesting another
name starts that context lazily and remains pending until the matching callback.

# External BeginFrames

External BeginFrames are a separate compositor clock. They are default-on on
macOS, where exact `ZZ_BROWSER_EXTERNAL_BEGIN_FRAME=0` is the kill switch, and
default-off on Linux/FreeBSD, where only exact
`ZZ_BROWSER_EXTERNAL_BEGIN_FRAME=1` opts in. Other platforms leave the feature
disabled. When enabled, session creation sets
`WindowInfo.external_begin_frame_enabled = 1`; CEF then produces compositor
frames only when `BrowserSession::send_external_begin_frame` calls the browser
host.

`BrowserController` drives every enabled visible session from message-pump
turns:

1. session creation, a fresh `FrameReady`, visibility, focus, or browser input
   marks the session hot for about 500 ms;
2. a new, newly visible, or newly hot pane sends immediately and anchors its next
   deadline at `now + interval`;
3. immediately before `do_message_loop_work`, a due pane sends exactly one
   BeginFrame, then advances its deadline by whole intervals until it is in the
   future; a late pump skips missed ticks instead of bursting, and never moves
   the long-run anchor to the late send time;
4. changing between hot and cold intervals, changing the ceiling, or selecting
   a new adaptive tier re-anchors from the current pump turn;
5. once cold, visible watchdog turns send a roughly 30 Hz keepalive so
   CSS/JavaScript animation can discover work;
6. hidden panes send no BeginFrames.

The creation/visibility activity window keeps BeginFrames flowing until the
first OSR paint. Linux/FreeBSD retain a 60 FPS display ceiling unless
`ZZ_BROWSER_FPS` is set; querying their actual display refresh rate remains
future work. Their opt-in default is deliberate because an invalid external
clock can leave a pane blank on an unvalidated display backend.

Hot panes can additionally use a per-pane adaptive divisor behind the exact
`ZZ_BROWSER_BF_ADAPTIVE=1` opt-in. Over roughly one-second samples the
controller compares BeginFrames sent with `FrameReady` deliveries. Sustained
delivery below 85% doubles the hot interval (halves the rate) by one tier,
down to 30 FPS when the configured ceiling permits. A throttled tier
delivering at least 95% for roughly two seconds probes one divisor faster.
Zero-delivery windows are ignored (an idle page says nothing about renderer
capability), and a pane going cold resets its divisor to the full ceiling.
The opt-in default is deliberate: frame delivery is demand-driven, so a
scroll pause inside the hot window still reads as a delivery shortfall and
downshifts a tier the renderer could sustain. With the earlier default-on
behavior this compounded into a one-way ratchet to the 30 FPS
floor. The signal needs to separate "renderer missed BeginFrames" from
"page had nothing to draw" before adaptation can default on.

Cadence stability matters more than the largest instantaneous frame count on a
fixed refresh grid. A renderer that cannot sustain a 240 Hz, 4.17 ms clock may
produce a higher but irregular off-grid rate; selecting 120 Hz instead lands
every frame on every second display tick. The anchored clock removes timer
beat patterns, while the adaptive divisor selects a stable grid-aligned tier
for heavy pages and still leaves light pages at the full ceiling.

This scheduler does not replace or modify `do_message_loop_work`. The external
message pump advances CEF's UI/task loop; external BeginFrames advance its
windowless compositor.

On macOS, `on_accelerated_paint` submits its Metal copy and returns without
waiting. The completion handler retains CEF's source `IOSurface` through the
blit, publishes only a completion newer than the last published sequence, and
routes asynchronous command-buffer failures through the same atomic readback
fallback used by synchronous import failures. See
[off-screen rendering](/browser/osr-rendering.md) for the bounded destination
pool.

# Handlers

`create_session(profile, ..., page_zoom_factor, frame_rate, ...)` selects the ready persistent
context for that name and builds a `BrowserClient` aggregating handlers wrapped by
`cef::wrap_*!` macros. Notable ones:

| Handler | Responsibility |
| --- | --- |
| `RenderHandlerBuilder` | `view_rect`/`screen_info`/`screen_point`, readback `on_paint`, and accelerated `on_accelerated_paint`. |
| `DisplayHandlerBuilder` | Address, title, and cursor change → `BrowserEvent`. |
| `LifeSpanHandlerBuilder` | `on_after_created`/`on_before_close`, popup cancel → `PopupRequested` event. |
| `LoadHandlerBuilder` | Loading-state and load-error events. |
| `RequestHandlerBuilder` | `on_before_browse`, open-URL-from-tab → `PopupRequested` event, renderer-terminated event. |
| `Denied{ContextMenu,Dialog,Download,Permission}Handler` | Cancel/deny privileged operations instead of falling through to an engine default. |
| `ImportCookieCallback` / `CookieImportFlushCallback` | Aggregate partial cookie-import results and flush the persistent store. |
| `SiteDataClearObserver` | Match the submitted DevTools message ID and complete current-origin site-data clearing. |

A CEF **message router** (`BrowserSideRouter`/`RendererSideRouter`) is created per
session and reused by the [element picker](/browser/element-picker.md).

# Examples

```rust
// Executable entry: dispatch subprocess or take the main runtime.
match browser_core::bootstrap()? {
    BrowserBootstrap::SubprocessExit(code) => std::process::exit(code),
    BrowserBootstrap::Runtime(runtime) => start_gpui_with(runtime),
}

// Dedicated helper/subprocess entry (macOS helper app):
std::process::exit(browser_core::run_subprocess());
```

```rust
pub enum RuntimeSignal {
    ContextInitialized,
    RequestContextInitialized { profile: Arc<str> },
    ScheduleMessagePump(i64), // delay in ms; a new positive delay replaces the pending wake
}
```

# Page zoom and OSR scale

CEF zoom levels are logarithmic (`ln(factor) / ln(1.2)`). `page_zoom.rs` owns the
Chrome-style 25–500% factor ladder, while `BrowserSession` stores the selected
factor for its lifetime. The effective factor passed to CEF is page zoom alone on
macOS/Windows/X11 and `page zoom × display scale` on the Wayland physical-OSR
path. Session creation, navigation callbacks, and viewport scale changes all
reapply that combined value so navigation cannot silently reset page zoom.

# No proxy preferences

The runtime writes no CEF preferences on a request context. `set_profile_proxy`
and `ensure_egress_profile_context` . the pair that pointed a remote pane's
composite context at a loopback CONNECT proxy with `bypass_list = <-loopback>` .
were deleted on 2026-08-01 with the QUIC tunnel behind them (see
[remote browser egress](/designs/remote-browser-egress.md)). Every context browses
directly from the client's network.

# Key files

| File | Role |
| --- | --- |
| `src/cef_runtime.rs` | Bootstrap, subprocess dispatch, `RuntimeApp`, detached main-thread command/pump handles, BeginFrame host calls, all CEF handlers, and session control. |
| `src/metal_osr.rs` | macOS Metal-IOSurface accelerated-frame producer. |
| `src/frame.rs` | Readback, Linux wgpu, and macOS `IOSurface` mailbox variants. |
| `src/lib.rs` | Re-exports `bootstrap`, `run_subprocess`, `BrowserRuntime`, `BrowserSession`, `BrowserCommandSink`, `BrowserError`, and `RuntimeSignal` under `cef-runtime`. |

# Related

- Advances through the [runtime & session lifecycle](/browser/lifecycle.md) state
  machines and creates [named private request contexts](/browser/profile.md).
- Feeds the [off-screen rendering pipeline](/browser/osr-rendering.md) via
  `on_paint`, and receives [translated input](/browser/input-translation.md).
- Part of the whole [zz-browser crate](/crates/zz-browser.md) and the
  [process model](/architecture/process-model.md).
- CEF version/artifact pins: [CEF artifacts](/references/cef-artifacts.md);
  bundling: [build a CEF bundle](/playbooks/build-cef-bundle.md).
- The wire and daemon halves behind the proxy preference:
  [remote browser egress](/designs/remote-browser-egress.md).
