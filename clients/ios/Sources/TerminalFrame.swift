import Combine
import Foundation

@MainActor
final class TerminalFrameSlot: ObservableObject {
    @Published private(set) var frame: TerminalFrame?

    init(frame: TerminalFrame? = nil) {
        self.frame = frame
    }

    func update(_ frame: TerminalFrame?) {
        guard self.frame !== frame else {
            return
        }
        self.frame = frame
    }
}

final class TerminalFrame {
    let pane: UInt64
    let columns: Int
    let rows: Int
    let generation: UInt64
    let viewGeneration: UInt64
    let dictionaryGeneration: UInt32
    let foreground: UInt32
    let background: UInt32
    let damage: TerminalDamage
    let cells: UnsafeBufferPointer<zz_cell>
    let styles: UnsafeBufferPointer<zz_style>
    let graphemeOffsets: UnsafeBufferPointer<UInt32>
    let graphemeBytes: UnsafeBufferPointer<UInt8>
    let cursor: zz_cursor?

    private let handle: OpaquePointer

    init?(client: OpaquePointer, pane: UInt64, damage: TerminalDamage) {
        guard let handle = zz_client_viewport_acquire(client, pane) else {
            return nil
        }
        let columns = Int(zz_viewport_columns(handle))
        let rows = Int(zz_viewport_rows(handle))
        let cellCount = columns * rows
        let styleCount = Int(zz_viewport_style_count(handle))
        let offsetCount = Int(zz_viewport_grapheme_offset_count(handle))
        let byteCount = Int(zz_viewport_grapheme_byte_count(handle))
        guard
            let cellPointer = zz_viewport_cells(handle),
            let stylePointer = zz_viewport_styles(handle),
            let offsetPointer = zz_viewport_grapheme_offsets(handle)
        else {
            zz_viewport_release(handle)
            return nil
        }
        if byteCount > 0 && zz_viewport_grapheme_bytes(handle) == nil {
            zz_viewport_release(handle)
            return nil
        }

        var cursorValue = zz_cursor()
        let cursor = zz_viewport_cursor(handle, &cursorValue) ? cursorValue : nil

        self.handle = handle
        self.pane = pane
        self.columns = columns
        self.rows = rows
        self.generation = zz_viewport_generation(handle)
        self.viewGeneration = zz_viewport_view_generation(handle)
        self.dictionaryGeneration = zz_viewport_dictionary_generation(handle)
        self.foreground = zz_viewport_foreground(handle)
        self.background = zz_viewport_background(handle)
        self.damage = damage
        self.cells = UnsafeBufferPointer(start: cellPointer, count: cellCount)
        self.styles = UnsafeBufferPointer(start: stylePointer, count: styleCount)
        self.graphemeOffsets = UnsafeBufferPointer(start: offsetPointer, count: offsetCount)
        self.graphemeBytes = UnsafeBufferPointer(
            start: zz_viewport_grapheme_bytes(handle),
            count: byteCount
        )
        self.cursor = cursor
    }

    deinit {
        zz_viewport_release(handle)
    }

    func glyph(at index: Int) -> String {
        guard cells.indices.contains(index) else {
            return ""
        }
        let raw = cells[index].glyph
        if raw == 0 {
            return ""
        }
        if raw & UInt32(ZZ_GRAPHEME_TABLE_BIT) == 0 {
            return UnicodeScalar(raw).map(String.init) ?? ""
        }
        let grapheme = Int(raw & ~UInt32(ZZ_GRAPHEME_TABLE_BIT))
        guard grapheme >= 0, grapheme + 1 < graphemeOffsets.count else {
            return ""
        }
        let start = Int(graphemeOffsets[grapheme])
        let end = Int(graphemeOffsets[grapheme + 1])
        guard start >= 0, end >= start, end <= graphemeBytes.count else {
            return ""
        }
        return String(decoding: graphemeBytes[start..<end], as: UTF8.self)
    }

    func style(for cell: zz_cell) -> zz_style? {
        let index = Int(cell.style)
        return styles.indices.contains(index) ? styles[index] : nil
    }
}
