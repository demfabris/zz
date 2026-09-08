import AppKit
import SwiftUI

public enum ZZInputContentType: Sendable {
    case text, search, password, url, email
}

public struct ZZTextField: View {
    @Environment(\.zzTheme) private var theme
    @Environment(\.isEnabled) private var isEnabled
    @FocusState private var focused: Bool
    @Binding private var text: String
    private let title: String
    private let placeholder: String
    private let size: ZZControlSize
    private let contentType: ZZInputContentType
    private let clearable: Bool
    private let leadingIcon: String?
    private let trailingIcon: String?
    private let prefix: String?
    private let suffix: String?
    private let isLoading: Bool
    private let invalid: Bool
    private let appearance: Bool
    private let bordered: Bool
    private let focusBordered: Bool
    private let alignment: TextAlignment
    private let externalFocus: FocusState<Bool>.Binding?
    private var customLeading: AnyView?
    private var customTrailing: AnyView?

    public init(
        _ title: String,
        text: Binding<String>,
        placeholder: String? = nil,
        size: ZZControlSize = .small,
        contentType: ZZInputContentType = .text,
        clearable: Bool = false,
        leadingIcon: String? = nil,
        trailingIcon: String? = nil,
        prefix: String? = nil,
        suffix: String? = nil,
        isLoading: Bool = false,
        invalid: Bool = false,
        appearance: Bool = true,
        bordered: Bool = true,
        focusBordered: Bool = true,
        alignment: TextAlignment = .leading,
        focus: FocusState<Bool>.Binding? = nil
    ) {
        self.title = title
        self._text = text
        self.placeholder = placeholder ?? title
        self.size = size
        self.contentType = contentType
        self.clearable = clearable
        self.leadingIcon = leadingIcon
        self.trailingIcon = trailingIcon
        self.prefix = prefix
        self.suffix = suffix
        self.isLoading = isLoading
        self.invalid = invalid
        self.appearance = appearance
        self.bordered = bordered
        self.focusBordered = focusBordered
        self.alignment = alignment
        self.externalFocus = focus
    }

    private var focus: FocusState<Bool>.Binding { externalFocus ?? $focused }

    public var body: some View {
        HStack(spacing: size == .large ? 8 : size == .medium ? 6 : 4) {
            if let customLeading { customLeading }
            if let icon = leadingIcon ?? (contentType == .search ? "magnifyingglass" : nil) {
                accessoryIcon(icon)
            }
            if let prefix {
                Text(prefix).foregroundStyle(theme.foreground.muted().color)
            }
            field
                .textFieldStyle(.plain)
                .focused(focus)
                .focusEffectDisabled()
                .multilineTextAlignment(alignment)
                .frame(maxWidth: .infinity)
                .accessibilityLabel(title)
                .accessibilityHint(invalid ? "Invalid value" : "")
            if isLoading {
                ZZSpinner(size: size.iconSize)
            } else if clearable && !text.isEmpty && isEnabled {
                Button {
                    text = ""
                    focus.wrappedValue = true
                } label: {
                    Image(systemName: "xmark.circle.fill")
                        .font(.system(size: 12))
                        .foregroundStyle(theme.foreground.muted().color)
                }
                .buttonStyle(ZZButtonStyle(variant: .ghost, size: .xSmall, flat: true, iconOnly: true))
                .focusable(false)
                .help("Clear \(title)")
                .accessibilityLabel("Clear \(title)")
            }
            if let suffix {
                Text(suffix).foregroundStyle(theme.foreground.muted().color)
            }
            if let customTrailing { customTrailing }
            if let trailingIcon { accessoryIcon(trailingIcon) }
        }
        .font(theme.font(size: size == .small ? 13 : size.fontSize))
        .foregroundStyle((isEnabled ? theme.foreground : theme.foreground.muted()).color)
        .padding(.horizontal, size.horizontalPadding)
        .frame(height: size.height)
        .modifier(
            ZZInputAppearance(
                appearance: appearance, bordered: bordered,
                focused: focus.wrappedValue && focusBordered && isEnabled, invalid: invalid
            )
        )
        .opacity(isEnabled ? 1 : 0.5)
    }

    public func leadingAccessory<Content: View>(@ViewBuilder _ content: () -> Content) -> Self {
        var field = self
        field.customLeading = AnyView(content())
        return field
    }

    public func trailingAccessory<Content: View>(@ViewBuilder _ content: () -> Content) -> Self {
        var field = self
        field.customTrailing = AnyView(content())
        return field
    }

    @ViewBuilder private var field: some View {
        if contentType == .password {
            SecureField(title, text: $text, prompt: Text(placeholder))
                .textContentType(.password)
        } else {
            TextField(title, text: $text, prompt: Text(placeholder))
                .textContentType(contentType == .url ? .URL : contentType == .email ? .emailAddress : nil)
                .autocorrectionDisabled(contentType != .text)
        }
    }

    private func accessoryIcon(_ icon: String) -> some View {
        Image(systemName: icon)
            .font(.system(size: size.iconSize))
            .foregroundStyle(theme.foreground.muted().color)
            .accessibilityHidden(true)
    }
}

private struct ZZInputAppearance: ViewModifier {
    @Environment(\.zzTheme) private var theme
    let appearance: Bool
    let bordered: Bool
    let focused: Bool
    let invalid: Bool

    @ViewBuilder func body(content: Content) -> some View {
        if !appearance {
            content
        } else if bordered {
            content.zzControlSurface(focused: focused, invalid: invalid)
        } else {
            content.background(theme.background.raised(1).color, in: ZZRoundedRectangle(radius: theme.radius))
        }
    }
}

public struct ZZTextEditor: View {
    @Environment(\.zzTheme) private var theme
    @Environment(\.isEnabled) private var isEnabled
    @FocusState private var focused: Bool
    @Binding private var text: String
    private let title: String
    private let placeholder: String
    private let size: ZZControlSize
    private let minRows: Int
    private let maxRows: Int
    private let invalid: Bool
    private let appearance: Bool
    private let monospaced: Bool

    public init(
        _ title: String,
        text: Binding<String>,
        placeholder: String? = nil,
        size: ZZControlSize = .small,
        minRows: Int = 3,
        maxRows: Int = 8,
        invalid: Bool = false,
        appearance: Bool = true,
        monospaced: Bool = false
    ) {
        self.title = title
        self._text = text
        self.placeholder = placeholder ?? title
        self.size = size
        self.minRows = max(1, minRows)
        self.maxRows = max(maxRows, max(1, minRows))
        self.invalid = invalid
        self.appearance = appearance
        self.monospaced = monospaced
    }

    public var body: some View {
        TextField(title, text: $text, prompt: Text(placeholder), axis: .vertical)
            .textFieldStyle(.plain)
            .lineLimit(minRows...maxRows)
            .font(theme.font(size: size == .small ? 13 : size.fontSize, monospaced: monospaced))
            .foregroundStyle(theme.foreground.color)
            .focused($focused)
            .focusEffectDisabled()
            .padding(.horizontal, size.horizontalPadding)
            .padding(.vertical, 8)
            .frame(maxWidth: .infinity, alignment: .leading)
            .modifier(
                ZZInputAppearance(
                    appearance: appearance, bordered: true, focused: focused && isEnabled, invalid: invalid)
            )
            .opacity(isEnabled ? 1 : 0.5)
            .accessibilityLabel(title)
            .accessibilityHint(invalid ? "Invalid value" : "")
    }
}

public struct ZZNumberRules: Equatable, Sendable {
    public let step: Double
    public let minimum: Double?
    public let maximum: Double?

    public init(step: Double = 1, minimum: Double? = nil, maximum: Double? = nil) {
        precondition(step.isFinite && step > 0)
        precondition(minimum == nil || minimum!.isFinite)
        precondition(maximum == nil || maximum!.isFinite)
        precondition(minimum == nil || maximum == nil || minimum! <= maximum!)
        self.step = step
        self.minimum = minimum
        self.maximum = maximum
    }

    public func acceptsPartial(_ value: String) -> Bool {
        var body = value[...]
        if body.first == "-" || body.first == "+" { body = body.dropFirst() }
        var seenDot = false
        for character in body {
            if character == "." && !seenDot {
                seenDot = true
            } else if !character.isASCII || !character.isNumber {
                return false
            }
        }
        return true
    }

    public func isValid(_ value: String) -> Bool {
        guard acceptsPartial(value), let number = Double(value), number.isFinite else { return false }
        let aboveMinimum = minimum.map { number >= $0 } ?? true
        let belowMaximum = maximum.map { number <= $0 } ?? true
        return aboveMinimum && belowMaximum
    }

    public func stepped(_ text: String, increment: Bool) -> String? {
        let current = text.trimmingCharacters(in: .whitespacesAndNewlines)
        let parsed = Double(current).flatMap { $0.isFinite ? $0 : nil }
        let base = parsed ?? 0
        var value = base + (increment ? step : -step)
        var digits = max(Self.fractionDigits(current), Self.decimalPrecision(step))
        if let minimum, value < minimum {
            value = minimum
            digits = max(digits, Self.decimalPrecision(minimum))
        }
        if let maximum, value > maximum {
            value = maximum
            digits = max(digits, Self.decimalPrecision(maximum))
        }
        guard value.isFinite else { return nil }
        if parsed != nil && (increment ? value <= base : value >= base) { return nil }
        return String(format: "%.*f", locale: Locale(identifier: "en_US_POSIX"), digits, value)
    }

    private static func fractionDigits(_ text: String) -> Int {
        text.split(separator: ".", omittingEmptySubsequences: false).dropFirst().first?.count ?? 0
    }

    private static func decimalPrecision(_ value: Double) -> Int {
        let parts = String(value).lowercased().split(separator: "e")
        let mantissa = String(parts[0])
        let exponent = parts.count > 1 ? Int(parts[1]) ?? 0 : 0
        let fraction = mantissa.split(separator: ".", omittingEmptySubsequences: false).dropFirst().first ?? ""
        let significantFraction = fraction.reversed().drop(while: { $0 == "0" }).count
        return max(0, significantFraction - exponent)
    }
}

public struct ZZNumberInput: View {
    @Environment(\.zzTheme) private var theme
    @Environment(\.isEnabled) private var isEnabled
    @Binding private var text: String
    @State private var focused = false
    private let title: String
    private let size: ZZControlSize
    private let rules: ZZNumberRules
    private let appearance: Bool

    public init(
        _ title: String,
        text: Binding<String>,
        step: Double = 1,
        minimum: Double? = nil,
        maximum: Double? = nil,
        size: ZZControlSize = .small,
        appearance: Bool = true
    ) {
        self.title = title
        self._text = text
        self.size = size
        self.rules = ZZNumberRules(step: step, minimum: minimum, maximum: maximum)
        self.appearance = appearance
    }

    public var body: some View {
        HStack(spacing: 0) {
            stepper(increment: false)
            ZZNativeNumberField(
                text: $text, focused: $focused, title: title, rules: rules,
                fontSize: size == .small ? 13 : size.fontSize, theme: theme, isEnabled: isEnabled,
                onStep: step
            )
            .accessibilityLabel(title)
            .accessibilityHint(rules.isValid(text) ? "" : "Enter a number within the allowed range")
            .accessibilityAdjustableAction { direction in
                switch direction {
                case .increment: step(increment: true)
                case .decrement: step(increment: false)
                @unknown default: break
                }
            }
            stepper(increment: true)
        }
        .font(theme.font(size: size == .small ? 13 : size.fontSize))
        .foregroundStyle(theme.foreground.color)
        .frame(height: size.height)
        .modifier(
            ZZInputAppearance(
                appearance: appearance, bordered: true, focused: focused && isEnabled,
                invalid: !text.isEmpty && !rules.isValid(text) && (!focused || !rules.acceptsPartial(text))
            )
        )
        .opacity(isEnabled ? 1 : 0.5)
    }

    private func stepper(increment: Bool) -> some View {
        Button {
            step(increment: increment)
            focused = true
        } label: {
            Image(systemName: increment ? "plus" : "minus")
                .font(.system(size: size.iconSize))
                .frame(width: size.height, height: size.height)
                .contentShape(.rect)
        }
        .buttonStyle(ZZButtonStyle(variant: .ghost, size: size, flat: true, iconOnly: true))
        .focusable(false)
        .disabled(rules.stepped(text, increment: increment) == nil)
        .accessibilityLabel("\(increment ? "Increase" : "Decrease") \(title)")
        .help(increment ? "Increase" : "Decrease")
    }

    private func step(increment: Bool) {
        guard isEnabled, let value = rules.stepped(text, increment: increment) else { return }
        text = value
    }
}

struct ZZNativeNumberField: NSViewRepresentable {
    @Binding var text: String
    @Binding var focused: Bool
    let title: String
    let rules: ZZNumberRules
    let fontSize: CGFloat
    let theme: ZZTheme
    let isEnabled: Bool
    let onStep: (Bool) -> Void

    func makeNSView(context: Context) -> NSTextField {
        let field = NSTextField()
        field.isBordered = false
        field.drawsBackground = false
        field.focusRingType = .none
        field.alignment = .center
        field.lineBreakMode = .byClipping
        field.maximumNumberOfLines = 1
        field.usesSingleLineMode = true
        field.setContentHuggingPriority(.defaultLow, for: .horizontal)
        field.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        field.delegate = context.coordinator
        return field
    }

    func updateNSView(_ field: NSTextField, context: Context) {
        context.coordinator.parent = self
        let editor = field.currentEditor() as? NSTextView
        if field.stringValue != text && editor?.hasMarkedText() != true {
            field.stringValue = text
        }
        field.font = theme.nsFont(size: fontSize)
        field.textColor = NSColor(theme.foreground.color)
        field.placeholderString = title
        field.isEnabled = isEnabled
        field.setAccessibilityLabel(title)
        if focused && field.currentEditor() == nil && isEnabled {
            field.window?.makeFirstResponder(field)
        }
    }

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    @MainActor final class Coordinator: NSObject, NSTextFieldDelegate {
        var parent: ZZNativeNumberField

        init(_ parent: ZZNativeNumberField) {
            self.parent = parent
        }

        func controlTextDidBeginEditing(_ notification: Notification) {
            if !parent.focused { parent.focused = true }
        }

        func controlTextDidEndEditing(_ notification: Notification) {
            controlTextDidChange(notification)
            if parent.focused { parent.focused = false }
        }

        func controlTextDidChange(_ notification: Notification) {
            guard let field = notification.object as? NSTextField else { return }
            let editor = field.currentEditor() as? NSTextView
            guard editor?.hasMarkedText() != true else { return }
            if parent.rules.acceptsPartial(field.stringValue) {
                parent.text = field.stringValue
            } else {
                let selection = editor?.selectedRange()
                field.stringValue = parent.text
                if let selection {
                    let location = min(selection.location, parent.text.utf16.count)
                    editor?.setSelectedRange(NSRange(location: location, length: 0))
                }
            }
        }

        func control(_ control: NSControl, textView: NSTextView, doCommandBy commandSelector: Selector) -> Bool {
            guard !textView.hasMarkedText(), parent.isEnabled else { return false }
            if commandSelector == #selector(NSResponder.moveUp(_:)) {
                parent.onStep(true)
                return true
            }
            if commandSelector == #selector(NSResponder.moveDown(_:)) {
                parent.onStep(false)
                return true
            }
            return false
        }
    }
}

public struct ZZSwitch: View {
    @Binding private var isOn: Bool
    private let title: String
    private let size: ZZControlSize
    private let showsLabel: Bool

    public init(_ title: String, isOn: Binding<Bool>, size: ZZControlSize = .medium, showsLabel: Bool = true) {
        self.title = title
        self._isOn = isOn
        self.size = size
        self.showsLabel = showsLabel
    }

    public var body: some View {
        Toggle(title, isOn: $isOn)
            .toggleStyle(ZZSwitchStyle(size: size, showsLabel: showsLabel))
    }
}

private struct ZZSwitchStyle: ToggleStyle {
    @Environment(\.zzTheme) private var theme
    @Environment(\.isEnabled) private var isEnabled
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @FocusState private var focused: Bool
    let size: ZZControlSize
    let showsLabel: Bool

    func makeBody(configuration: Configuration) -> some View {
        let compact = size == .xSmall || size == .small
        let width: CGFloat = compact ? 28 : 36
        let height: CGFloat = compact ? 16 : 20
        let thumb: CGFloat = compact ? 12 : 16
        let track = configuration.isOn ? theme.foreground.opacity(isEnabled ? 1 : 0.5) : theme.background.raised(3)
        Button {
            configuration.isOn.toggle()
        } label: {
            HStack(spacing: 8) {
                if showsLabel {
                    configuration.label
                        .font(theme.font(size: size.fontSize))
                        .foregroundStyle(theme.foreground.color)
                }
                Capsule()
                    .fill(track.color)
                    .frame(width: width, height: height)
                    .overlay(alignment: .leading) {
                        Circle()
                            .fill(theme.background.opacity(isEnabled ? 1 : 0.35).color)
                            .frame(width: thumb, height: thumb)
                            .shadow(
                                color: theme.scrim.opacity(theme.shadows ? 0.2 * theme.shadowStrength : 0).color,
                                radius: 1, y: 1
                            )
                            .padding(2)
                            .offset(x: configuration.isOn ? width - thumb - 4 : 0)
                    }
                    .overlay {
                        Capsule().strokeBorder(focused ? theme.foreground.color : .clear, lineWidth: 1)
                            .padding(-3)
                    }
                    .animation(reduceMotion ? nil : .easeInOut(duration: 0.15), value: configuration.isOn)
            }
            .contentShape(.rect)
        }
        .buttonStyle(.plain)
        .focused($focused)
        .focusEffectDisabled()
        .accessibilityRepresentation {
            Toggle(isOn: configuration.$isOn) { configuration.label }.toggleStyle(.switch)
        }
    }
}
