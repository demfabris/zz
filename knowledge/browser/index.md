<!-- okf:listing:start (managed by okf.py index — edit prose outside this fence) -->
# Concepts

* [CEF runtime & subprocess dispatch](cef-runtime.md) - CEF Alloy OSR bootstrap, single-binary subprocess dispatch, frame-rate policy, external BeginFrames, message pumping, and safe foreground command dispatch.
* [In-page element picker](element-picker.md) - A token-guarded, single-use overlay that lets the user pick a DOM element in the page and returns a bounded, sanitized source-context string plus an optional screenshot of the picked area.
* [Input translation (GPUI → CEF)](input-translation.md) - Browser-neutral pointer/wheel/keyboard/text/IME input, ordered GPUI-to-CEF dispatch, pane shortcuts, and address-field URL normalization plus omnibox search fallback.
* [Browser runtime & session lifecycle](lifecycle.md) - Runtime/profile-context/session state machines and the browser-neutral events that CEF callbacks translate into.
* [Off-screen rendering & the frame mailbox](osr-rendering.md) - How CEF frames cross the one-slot mailbox through the universal readback tier, Linux wgpu tier, macOS Metal-IOSurface tier, or Windows D3D11 tier, and how zz paces visible sessions.
* [Named zz profiles & persistent request contexts](profile.md) - Named zz-owned CEF profiles isolate browser state on the client's disk, and explicit Chrome cookie and history import uses bounded read-only snapshots.
<!-- okf:listing:end -->
