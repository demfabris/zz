import CZZClient
import Foundation
import Observation

@Observable @MainActor
public final class TerminalSlot {
    public var frame: TerminalFrame?
}

public final class TerminalFrame {
    public let handle: OpaquePointer
    public let columns: Int
    public let rows: Int
    public let cells: UnsafeBufferPointer<zz_cell>
    public let styles: UnsafeBufferPointer<zz_style>
    public let foreground: UInt32
    public let background: UInt32
    public let cursor: zz_cursor?
    private let offsets: UnsafeBufferPointer<UInt32>
    private let bytes: UnsafeBufferPointer<UInt8>

    init?(client: OpaquePointer, pane: UInt64) {
        guard let handle = zz_client_viewport_acquire(client, pane) else { return nil }
        self.handle = handle
        columns = Int(zz_viewport_columns(handle))
        rows = Int(zz_viewport_rows(handle))
        cells = UnsafeBufferPointer(start: zz_viewport_cells(handle), count: columns * rows)
        styles = UnsafeBufferPointer(start: zz_viewport_styles(handle), count: zz_viewport_style_count(handle))
        offsets = UnsafeBufferPointer(
            start: zz_viewport_grapheme_offsets(handle), count: zz_viewport_grapheme_offset_count(handle))
        bytes = UnsafeBufferPointer(
            start: zz_viewport_grapheme_bytes(handle), count: zz_viewport_grapheme_byte_count(handle))
        foreground = zz_viewport_foreground(handle)
        background = zz_viewport_background(handle)
        var value = zz_cursor()
        cursor = zz_viewport_cursor(handle, &value) ? value : nil
    }

    deinit { zz_viewport_release(handle) }

    public func glyph(at index: Int) -> String {
        guard cells.indices.contains(index) else { return "" }
        let raw = cells[index].glyph
        guard raw != 0 else { return "" }
        guard raw & UInt32(ZZ_GRAPHEME_TABLE_BIT) != 0 else {
            return UnicodeScalar(raw).map(String.init) ?? ""
        }
        let index = Int(raw & ~UInt32(ZZ_GRAPHEME_TABLE_BIT))
        guard index + 1 < offsets.count else { return "" }
        let start = Int(offsets[index])
        let end = Int(offsets[index + 1])
        guard start <= end, end <= bytes.count else { return "" }
        return String(decoding: bytes[start..<end], as: UTF8.self)
    }

    public var text: String {
        (0..<rows).map { row in
            (0..<columns).map { column in
                let index = row * columns + column
                guard cells[index].flags & UInt16(ZZ_CELL_WIDTH_MASK) < 2 else { return "" }
                let glyph = glyph(at: index)
                return glyph.isEmpty ? " " : glyph
            }.joined()
        }.joined(separator: "\n")
    }
}
