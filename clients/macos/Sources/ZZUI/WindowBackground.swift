import AppKit
import SwiftUI

public struct ZZWindowBackground: NSViewRepresentable {
    private let tint: ZZColor?
    private let enabled: Bool

    public init(tint: ZZColor? = nil, enabled: Bool = true) {
        self.tint = tint
        self.enabled = enabled
    }

    public func makeNSView(context: Context) -> NSVisualEffectView {
        let view = ZZWindowEffectView()
        view.setTint(tint)
        view.state = enabled ? .active : .inactive
        return view
    }

    public func updateNSView(_ view: NSVisualEffectView, context: Context) {
        (view as? ZZWindowEffectView)?.setTint(tint)
        view.state = enabled ? .active : .inactive
    }
}

private final class ZZWindowEffectView: NSVisualEffectView {
    private let tintView = NSView()

    init() {
        super.init(frame: .zero)
        material = .underWindowBackground
        blendingMode = .behindWindow
        state = .followsWindowActiveState
        setAccessibilityElement(false)
        tintView.frame = bounds
        tintView.autoresizingMask = [.width, .height]
        tintView.wantsLayer = true
        tintView.setAccessibilityElement(false)
        addSubview(tintView)
    }

    required init?(coder: NSCoder) { nil }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        window?.isOpaque = false
        window?.backgroundColor = .clear
        window?.titlebarAppearsTransparent = true
    }

    override func hitTest(_ point: NSPoint) -> NSView? { nil }

    func setTint(_ tint: ZZColor?) {
        tintView.layer?.backgroundColor = tint?.nsColor.cgColor
        tintView.isHidden = tint == nil
    }
}
