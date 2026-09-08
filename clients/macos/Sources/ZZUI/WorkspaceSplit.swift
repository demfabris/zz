import AppKit
import SwiftUI

public struct ZZWorkspaceSplit<First: View, Second: View>: View {
    @Binding private var fraction: CGFloat
    @GestureState private var dragStartFraction: CGFloat?
    private let axis: ZZPaneSplitAxis
    private let dividerWidth: CGFloat
    private let first: First
    private let second: Second

    public init(
        axis: ZZPaneSplitAxis, fraction: Binding<CGFloat>, dividerWidth: CGFloat = 6,
        @ViewBuilder first: () -> First, @ViewBuilder second: () -> Second
    ) {
        self.axis = axis
        self._fraction = fraction
        self.dividerWidth = max(0, dividerWidth)
        self.first = first()
        self.second = second()
    }

    public var body: some View {
        GeometryReader { geometry in
            let span = axis == .horizontal ? geometry.size.width : geometry.size.height
            let gutter = min(dividerWidth, max(0, span))
            let available = max(0, span - gutter)
            let firstSpan = available * Self.clamped(fraction)
            let secondSpan = available - firstSpan

            if axis == .horizontal {
                HStack(spacing: 0) {
                    first.frame(width: firstSpan, height: geometry.size.height).clipped()
                    divider(available: available).frame(width: gutter, height: geometry.size.height)
                    second.frame(width: secondSpan, height: geometry.size.height).clipped()
                }
            } else {
                VStack(spacing: 0) {
                    first.frame(width: geometry.size.width, height: firstSpan).clipped()
                    divider(available: available).frame(width: geometry.size.width, height: gutter)
                    second.frame(width: geometry.size.width, height: secondSpan).clipped()
                }
            }
        }
    }

    private func divider(available: CGFloat) -> some View {
        Color.clear
            .contentShape(.rect)
            .gesture(
                DragGesture(minimumDistance: 0, coordinateSpace: .global)
                    .updating($dragStartFraction) { _, start, _ in
                        if start == nil { start = Self.clamped(fraction) }
                    }
                    .onChanged { value in
                        guard available > 0 else { return }
                        let translation = axis == .horizontal ? value.translation.width : value.translation.height
                        let start = dragStartFraction ?? Self.clamped(fraction)
                        fraction = Self.clamped(start + translation / available)
                    }
            )
            .onHover { hovering in
                let cursor = axis == .horizontal ? NSCursor.resizeLeftRight : NSCursor.resizeUpDown
                (hovering ? cursor : NSCursor.arrow).set()
            }
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(axis == .horizontal ? "Resize panes horizontally" : "Resize panes vertically")
            .accessibilityValue("First pane \(Int((Self.clamped(fraction) * 100).rounded())) percent")
            .accessibilityAdjustableAction { direction in
                switch direction {
                case .increment: fraction = Self.clamped(fraction + 0.05)
                case .decrement: fraction = Self.clamped(fraction - 0.05)
                @unknown default: break
                }
            }
    }

    private static func clamped(_ value: CGFloat) -> CGFloat {
        value.isFinite ? min(0.9, max(0.1, value)) : 0.5
    }
}
