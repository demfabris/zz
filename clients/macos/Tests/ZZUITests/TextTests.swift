import AppKit
import SwiftUI
import Testing

@testable import ZZUI

struct TextTests {
    @Test func rustByteRangesConvertToNativeUTF16() {
        let source = "a🦀é let"
        let span = ZZSyntaxSpan(utf8Range: 8..<11, in: source, kind: .keyword)
        #expect(span?.range == NSRange(location: 5, length: 3))
        #expect(ZZSyntaxSpan(utf8Range: 2..<4, in: source, kind: .keyword) == nil)
        #expect(ZZSyntaxSpan(utf8Range: 0..<50, in: source, kind: .keyword) == nil)
    }

    @Test func nativeSelectionsClampWithoutOverflow() {
        #expect(
            ZZTextSelection.clamped(NSRange(location: 40, length: Int.max), toUTF16Length: 8)
                == NSRange(location: 8, length: 0))
        #expect(
            ZZTextSelection.clamped(NSRange(location: 3, length: Int.max), toUTF16Length: 8)
                == NSRange(location: 3, length: 5))
        #expect(
            ZZTextSelection.clamped(NSRange(location: NSNotFound, length: 0), toUTF16Length: 0)
                == NSRange(location: 0, length: 0))
    }

    @Test func nativeLineNumbersCountUTF16AndCRLF() {
        #expect(ZZTextSelection.lineStarts(in: "") == [0])
        #expect(ZZTextSelection.lineStarts(in: "🦀\r\nabc\n") == [0, 4, 8])
        #expect(ZZTextSelection.lineStarts(in: "one\ntwo") == [0, 4])
    }

    @Test @MainActor func markdownKeepsNestedCodeAndInlineFormatting() {
        let document = ZZMarkdownDocument(
            "- **One**\n\n  ```rust\n  fn main() {}\n  ```\n\n## Heading\n\n[A link](https://zzmux.sh)")
        #expect(
            document.blocks.contains {
                $0.intents.contains {
                    if case .codeBlock = $0.kind { return true }
                    return false
                }
            })
        let rendered = document.attributedString(theme: .dark)
        #expect(rendered.string.contains("•\tOne"))
        #expect(rendered.string.contains("fn main() {}"))
        #expect(rendered.string.contains("Heading\n"))
        let link = (rendered.string as NSString).range(of: "A link")
        #expect(
            rendered.attribute(.link, at: link.location, effectiveRange: nil) as? URL
                == URL(string: "https://zzmux.sh")
        )
    }

    @Test @MainActor func markdownTableCreatesNativeTableCells() {
        let text = ZZMarkdownDocument("| A | B |\n| - | - |\n| C | D |").attributedString(theme: .dark)
        let range = (text.string as NSString).range(of: "D")
        let paragraph =
            text.attribute(.paragraphStyle, at: range.location, effectiveRange: nil) as? NSParagraphStyle
        let cell = paragraph?.textBlocks.first as? NSTextTableBlock
        #expect(cell?.startingRow == 1)
        #expect(cell?.startingColumn == 1)
        #expect(cell?.table.numberOfColumns == 2)
    }

    @Test @MainActor func nestedMixedListsKeepTheirOwnMarkers() {
        let text = ZZMarkdownDocument("1. Outer\n   - Inner\n   - Another\n2. Last").attributedString(
            theme: .dark
        )
        .string
        #expect(text.contains("1.\tOuter"))
        #expect(text.contains("◦\tInner"))
        #expect(text.contains("◦\tAnother"))
        #expect(text.contains("2.\tLast"))
    }

    @Test @MainActor func markdownTaskItemsRenderCheckedAndUncheckedStates() {
        let text = ZZMarkdownDocument("- [x] Done\n- [ ] Next").attributedString(theme: .dark).string
        #expect(text.contains("☑\tDone"))
        #expect(text.contains("☐\tNext"))
    }

    @Test @MainActor func markdownImagesKeepSelectionOffsetsWhenLoaded() {
        let url = URL(string: "https://example.com/image.png")!
        let document = ZZMarkdownDocument(
            "Before\n\n![A workspace](https://example.com/image.png)\n\nAfter")
        let placeholder = document.attributedString(theme: .dark)
        let image = NSImage(size: NSSize(width: 1200, height: 600))
        let loaded = document.attributedString(theme: .dark, images: [url: image])
        #expect(document.imageURLs == [url])
        #expect(placeholder.string == loaded.string)
        let range = (loaded.string as NSString).range(of: "\u{fffc}")
        let attachment =
            loaded.attribute(.attachment, at: range.location, effectiveRange: nil) as? NSTextAttachment
        #expect(attachment?.image?.accessibilityDescription == "A workspace")
        let bounds = attachment?.attachmentBounds(
            for: nil, proposedLineFragment: NSRect(x: 0, y: 0, width: 320, height: 400),
            glyphPosition: .zero,
            characterIndex: range.location)
        #expect(bounds?.width == 320)
        #expect(bounds?.height == 160)
    }

    @Test @MainActor func codeIndentationPreservesUnicodeAndSupportsUndo() {
        let view = ZZNativeCodeTextView(frame: .zero)
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 400, height: 200), styleMask: [.borderless],
            backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.contentView = view
        view.allowsUndo = true
        view.string = "    let crab = \"🦀\""
        view.setSelectedRange(NSRange(location: view.string.utf16.count, length: 0))
        view.insertNewline(nil)
        #expect(view.string == "    let crab = \"🦀\"\n    ")
        #expect(view.undoManager?.canUndo == true)
        view.undoManager?.undo()
        #expect(view.string == "    let crab = \"🦀\"")
    }

    @Test @MainActor func codeEditorUsesWindowUndoAfterBindingUpdates() async throws {
        let state = EditorBindingState()
        let host = NSHostingView(rootView: EditorBindingFixture(state: state))
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 700, height: 300), styleMask: [.borderless],
            backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.contentView = host
        host.layoutSubtreeIfNeeded()
        try await Task.sleep(for: .milliseconds(40))
        func textViews(in view: NSView) -> [ZZNativeCodeTextView] {
            if let text = view as? ZZNativeCodeTextView { return [text] }
            return view.subviews.flatMap(textViews)
        }
        let editor = try #require(textViews(in: host).first)
        window.makeFirstResponder(editor)
        editor.selectAll(nil)
        let replacement = "let crab = \"🦀\";\nlet panes = 2;"
        editor.insertText(replacement, replacementRange: editor.selectedRange())
        try await Task.sleep(for: .milliseconds(40))
        host.layoutSubtreeIfNeeded()
        #expect(editor.undoManager === window.undoManager)
        #expect(window.undoManager?.canUndo == true)
        window.undoManager?.undo()
        #expect(editor.string == "initial")
        #expect(state.text == "initial")
        try await Task.sleep(for: .milliseconds(40))
        #expect(window.undoManager?.canRedo == true)
        window.undoManager?.redo()
        #expect(editor.string == replacement)
        #expect(state.text == replacement)
    }

    @Test @MainActor func externalCodeReplacementPreservesOtherEditorsUndo() async throws {
        let state = EditorBindingState()
        let host = NSHostingView(rootView: EditorBindingFixture(state: state, includesOtherEditor: true))
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 700, height: 300), styleMask: [.borderless],
            backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.contentView = host
        host.layoutSubtreeIfNeeded()
        try await Task.sleep(for: .milliseconds(40))
        func textViews(in view: NSView) -> [ZZNativeCodeTextView] {
            if let text = view as? ZZNativeCodeTextView { return [text] }
            return view.subviews.flatMap(textViews)
        }
        let views = textViews(in: host)
        let editor = try #require(views.first { $0.string == "initial" })
        let otherEditor = try #require(views.first { $0.string == "other" })
        editor.insertText("changed", replacementRange: NSRange(location: 0, length: editor.string.utf16.count))
        otherEditor.insertText(
            "other changed", replacementRange: NSRange(location: 0, length: otherEditor.string.utf16.count))
        try await Task.sleep(for: .milliseconds(40))
        state.text = "external replacement"
        try await Task.sleep(for: .milliseconds(40))
        host.layoutSubtreeIfNeeded()
        #expect(editor.string == "external replacement")
        #expect(window.undoManager?.canUndo == true)
        window.undoManager?.undo()
        #expect(editor.string == "external replacement")
        #expect(state.text == "external replacement")
        #expect(otherEditor.string == "other")
        #expect(state.otherText == "other")
    }

    @Test @MainActor func codeTabIndentsSelectedLinesAndBacktabRestoresThem() {
        let view = ZZNativeCodeTextView(frame: .zero)
        view.allowsUndo = true
        let source = "🦀 first\nsecond\nthird"
        view.string = source
        view.setSelectedRange(NSRange(location: 0, length: "🦀 first\nsecond\n".utf16.count))
        view.insertTab(nil)
        #expect(view.string == "  🦀 first\n  second\nthird")
        #expect(
            view.selectedRange() == NSRange(location: 0, length: "  🦀 first\n  second\n".utf16.count))
        view.insertBacktab(nil)
        #expect(view.string == source)
        #expect(view.selectedRange() == NSRange(location: 0, length: 0))
    }

    @Test @MainActor func codeTabDefaultsToTwoSpacesAndCanInsertHardTabs() {
        let view = ZZNativeCodeTextView(frame: .zero)
        view.insertTab(nil)
        #expect(view.string == "  ")
        view.hardTabs = true
        view.insertTab(nil)
        #expect(view.string == "  \t")
    }

    @Test @MainActor func lineNumberBackgroundStaysInsideRulerBounds() throws {
        let bitmap = try #require(
            NSBitmapImageRep(
                bitmapDataPlanes: nil, pixelsWide: 160, pixelsHigh: 160, bitsPerSample: 8,
                samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
                bytesPerRow: 0, bitsPerPixel: 0))
        let context = try #require(NSGraphicsContext(bitmapImageRep: bitmap))
        let ruler = ZZLineNumberRuler(scrollView: nil, orientation: .verticalRuler)
        ruler.frame = NSRect(x: 0, y: 0, width: 38, height: 100)
        ruler.background = .red
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = context
        context.cgContext.clear(CGRect(x: 0, y: 0, width: 160, height: 160))
        context.cgContext.translateBy(x: 40, y: 40)
        ruler.drawHashMarksAndLabels(in: NSRect(x: -40, y: -40, width: 160, height: 160))
        NSGraphicsContext.restoreGraphicsState()
        #expect(bitmap.colorAt(x: 10, y: 10)?.alphaComponent == 0)
        #expect(bitmap.colorAt(x: 50, y: 80)?.redComponent == 1)
    }

    @Test @MainActor func keyedToastsReplaceAndCancelPriorDismissal() async throws {
        let center = ZZToastCenter()
        let first = center.show("Connecting", key: "host", duration: .milliseconds(5))
        let replacement = center.show("Connected", key: "host", duration: nil)
        #expect(first != replacement)
        try await Task.sleep(for: .milliseconds(20))
        #expect(center.toasts.map(\.id) == [replacement])
        center.dismiss(key: "host")
        #expect(center.toasts.isEmpty)
    }

    @Test @MainActor func toastBurstsShowOnlyTheLatestTenInArrivalOrder() {
        let center = ZZToastCenter()
        for index in 0..<15 { center.show(String(index), duration: nil) }
        #expect(center.visibleToasts.map(\.message) == (5..<15).map(String.init))
        #expect(center.toasts.count == 15)
        center.dismissAll()
        #expect(center.visibleToasts.isEmpty)
    }

    @Test @MainActor func customToastContentReleasesAfterKeyedReplacement() {
        let center = ZZToastCenter()
        weak var retained: ToastContentProbe?
        do {
            let probe = ToastContentProbe()
            retained = probe
            center.show("Archived", key: "archive", duration: nil) { _ in
                ToastContentProbeView(probe: probe)
            }
        }
        #expect(retained != nil)
        center.show("Replaced", key: "archive", duration: nil)
        #expect(retained == nil)
        center.dismissAll()
    }

    @Test @MainActor func markdownUsesFinalNativeWidthInsideNestedTimeline() async throws {
        let source = String(
            repeating: "Native text should wrap at the width of its message. ", count: 6)
        let host = NSHostingView(
            rootView:
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 16) {
                        ZZAgentUserMessage(source)
                        ZZAgentPlan { ZZMarkdown(source) }
                    }
                    .frame(maxWidth: 680).frame(maxWidth: .infinity)
                }
        )
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 800, height: 700), styleMask: [.borderless],
            backing: .buffered,
            defer: false)
        window.contentView = host
        host.layoutSubtreeIfNeeded()
        try await Task.sleep(for: .milliseconds(40))
        host.layoutSubtreeIfNeeded()
        func textViews(in view: NSView) -> [NSTextView] {
            if let text = view as? NSTextView { return [text] }
            return view.subviews.flatMap(textViews)
        }
        let views = textViews(in: host)
        #expect(views.count == 2)
        for view in views {
            #expect(view.bounds.width > 500)
            #expect(abs((view.textContainer?.containerSize.width ?? 0) - view.bounds.width) < 1)
            if let layout = view.layoutManager, let container = view.textContainer {
                layout.ensureLayout(for: container)
                #expect(layout.usedRect(for: container).height <= view.bounds.height)
            }
        }
    }
}

private final class ToastContentProbe {}

@MainActor @Observable private final class EditorBindingState {
    var text = "initial"
    var otherText = "other"
}

private struct EditorBindingFixture: View {
    @Bindable var state: EditorBindingState
    var includesOtherEditor = false

    var body: some View {
        HStack {
            ZZCodeEditor(text: $state.text)
            if includesOtherEditor { ZZCodeEditor(text: $state.otherText) }
        }
    }
}

private struct ToastContentProbeView: View {
    let probe: ToastContentProbe
    var body: some View { Text("Undo") }
}
