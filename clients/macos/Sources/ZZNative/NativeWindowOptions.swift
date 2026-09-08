import AppKit
import SwiftUI
import ZZNativeCore

struct NativeWindowOptions: NSViewRepresentable {
    let settings: NativeSettings
    let dark: Bool
    func makeNSView(context: Context) -> NativeWindowOptionsView { NativeWindowOptionsView() }
    func updateNSView(_ view: NativeWindowOptionsView, context: Context) {
        view.systemTitlebar = settings.bool("use-system-titlebar")
        view.radius = settings.number("window-corner-radius", fallback: 13.5)
        view.updateWindow()
        let icon = settings.text("app-icon")
        let iconDark = icon == "dark" || (icon == "automatic" && dark)
        if let url = Bundle.main.url(forResource: iconDark ? "zz-dark-512" : "zz-light-512", withExtension: "png"),
            let image = NSImage(contentsOf: url)
        {
            NSApp.applicationIconImage = image
        }
    }
}

final class NativeWindowOptionsView: NSView {
    var systemTitlebar = false
    var radius: CGFloat = 13.5
    override func hitTest(_ point: NSPoint) -> NSView? { nil }
    override func viewDidMoveToWindow() { super.viewDidMoveToWindow(); updateWindow() }
    func updateWindow() {
        guard let window else { return }
        if systemTitlebar == window.styleMask.contains(.fullSizeContentView) {
            if systemTitlebar {
                window.styleMask.remove(.fullSizeContentView)
            } else {
                window.styleMask.insert(.fullSizeContentView)
            }
        }
        window.titleVisibility = systemTitlebar ? .visible : .hidden
        window.titlebarAppearsTransparent = !systemTitlebar
        window.contentView?.wantsLayer = true
        window.contentView?.layer?.cornerRadius = radius
        window.contentView?.layer?.masksToBounds = true
    }
}
