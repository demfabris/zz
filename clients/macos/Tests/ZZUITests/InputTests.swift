import AppKit
import SwiftUI
import Testing

@testable import ZZUI

@Test func numberValidationAllowsIntermediateEditingButRejectsNonNumericText() {
    let rules = ZZNumberRules(minimum: -10, maximum: 10)
    for value in ["", "-", "+", ".", "1", "-1", "1.", "1.5", "0.05", "256"] {
        #expect(rules.acceptsPartial(value))
    }
    for value in ["a", "1a", "1.2.3", "1 2", "--1", "1-", "1e5", "١", "NaN", "inf"] {
        #expect(!rules.acceptsPartial(value))
    }
    #expect(rules.isValid("-10"))
    #expect(rules.isValid("10"))
    #expect(!rules.isValid("11"))
    #expect(!rules.isValid("-11"))
    #expect(!rules.isValid("-"))
}

@Test func decimalSteppingPreservesPrecisionAndBounds() {
    #expect(ZZNumberRules(step: 0.2).stepped("0.1", increment: true) == "0.3")
    #expect(ZZNumberRules(step: 0.05).stepped("1", increment: true) == "1.05")
    #expect(ZZNumberRules().stepped("4", increment: false) == "3")
    #expect(ZZNumberRules(step: 0.05).stepped("1.000", increment: true) == "1.050")
    let rules = ZZNumberRules(step: 4, minimum: 0, maximum: 256)
    #expect(rules.stepped("255", increment: true) == "256")
    #expect(rules.stepped("2", increment: false) == "0")
    #expect(rules.stepped("256", increment: true) == nil)
    #expect(rules.stepped("0", increment: false) == nil)
    #expect(rules.stepped("-5", increment: false) == nil)
    #expect(ZZNumberRules(minimum: 10).stepped("", increment: true) == "10")
    #expect(ZZNumberRules().stepped("", increment: true) == "1")
    #expect(ZZNumberRules(step: 0.01, maximum: 0.125).stepped("0.12", increment: true) == "0.125")
    #expect(ZZNumberRules(step: 1e-20).stepped("0", increment: true) == "0.00000000000000000001")
}

@Test func selectSearchKeepsOptionIdentityAndSkipsDisabledRows() {
    let options: [ZZSelectOption] = [
        .init("rust", title: "Rust", group: "Systems", detail: "Shared client core"),
        .init("swift", title: "Swift", group: "Systems", disabled: true),
        .init("cafe", title: "Café"),
    ]
    #expect(ZZSelectOption.matching("SYSTEMS rust", in: options).map(\.id) == ["rust"])
    #expect(ZZSelectOption.matching("cafe", in: options).map(\.id) == ["cafe"])
    #expect(ZZSelectOption.matching("client", in: options).map(\.id) == ["rust"])
    #expect(ZZSelectOption.matching("unknown", in: options).isEmpty)
    #expect(ZZSelectOption.next(after: "rust", forward: true, in: options) == "cafe")
    #expect(ZZSelectOption.next(after: "cafe", forward: true, in: options) == "rust")
    #expect(ZZSelectOption.next(after: "rust", forward: false, in: options) == "cafe")
    #expect(ZZSelectOption.next(after: nil, forward: false, in: options) == "cafe")
    #expect(ZZSelectOption.next(after: nil, forward: true, in: [.init("off", disabled: true)]) == nil)
    #expect(ZZSelectOption.next(after: nil, forward: true, in: []) == nil)
}

@Test @MainActor func colorOverrideClearsAndRejectsInvalidEdits() {
    let previous = ZZColor(hex: "#7aa2f7")
    #expect(ZZColorPicker.committedColor(for: "", preserving: previous) == nil)
    #expect(ZZColorPicker.committedColor(for: " \n ", preserving: previous) == nil)
    #expect(ZZColorPicker.committedColor(for: "#garbage", preserving: previous) == previous)
    #expect(ZZColorPicker.committedColor(for: "#abc", preserving: previous)?.hex == "#aabbcc")
    #expect(ZZColorPicker.committedColor(for: "#11223344", preserving: previous)?.hex == "#11223344")
    #expect(ZZColorPicker.presets.count == 40)
}

@Test @MainActor func nativeNumberEditsRejectInvalidTextWithoutChangingMarkedText() throws {
    _ = NSApplication.shared
    var text = "12"
    var focused = false
    let view = ZZNativeNumberField(
        text: Binding(get: { text }, set: { text = $0 }),
        focused: Binding(get: { focused }, set: { focused = $0 }),
        title: "Number", rules: ZZNumberRules(), fontSize: 13, theme: .light,
        isEnabled: true, onStep: { _ in }
    )
    let coordinator = ZZNativeNumberField.Coordinator(view)
    let window = NSWindow(
        contentRect: NSRect(x: 0, y: 0, width: 300, height: 100),
        styleMask: [.titled], backing: .buffered, defer: false
    )
    window.isReleasedWhenClosed = false
    defer { window.close() }
    let field = NSTextField(frame: NSRect(x: 20, y: 20, width: 200, height: 28))
    field.delegate = coordinator
    field.stringValue = text
    window.contentView?.addSubview(field)
    #expect(window.makeFirstResponder(field))
    let editor = try #require(field.currentEditor() as? NSTextView)

    editor.insertText("a", replacementRange: NSRange(location: 2, length: 0))
    #expect(text == "12")
    #expect(field.stringValue == "12")

    editor.setMarkedText(
        "に", selectedRange: NSRange(location: 1, length: 0), replacementRange: NSRange(location: 0, length: 2))
    #expect(editor.hasMarkedText())
    #expect(text == "12")
    #expect(field.stringValue == "に")

    editor.insertText("に", replacementRange: editor.markedRange())
    #expect(!editor.hasMarkedText())
    #expect(text == "12")
    #expect(field.stringValue == "12")

    editor.insertText("12.5", replacementRange: NSRange(location: 0, length: editor.string.utf16.count))
    #expect(text == "12.5")
    #expect(field.stringValue == "12.5")
}
