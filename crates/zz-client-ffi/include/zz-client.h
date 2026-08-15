/* The C contract over the zz-client core. Hand-maintained; the smoke test
 * compiles and links a C client against this header, so drift from the Rust
 * exports in src/ffi.rs fails the build. Unix only. */

#ifndef ZZ_CLIENT_H
#define ZZ_CLIENT_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct zz_client zz_client;
typedef struct zz_mux_snapshot zz_mux_snapshot;
typedef struct zz_viewport zz_viewport;

#define ZZ_GRAPHEME_TABLE_BIT (1u << 31)
#define ZZ_NO_COLOR UINT32_MAX
#define ZZ_CELL_WIDTH_MASK 3u
#define ZZ_ATTR_BOLD (1u << 0)
#define ZZ_ATTR_ITALIC (1u << 1)
#define ZZ_ATTR_FAINT (1u << 2)
#define ZZ_ATTR_BLINK (1u << 3)
#define ZZ_ATTR_INVISIBLE (1u << 4)
#define ZZ_ATTR_STRIKETHROUGH (1u << 5)
#define ZZ_ATTR_OVERLINE (1u << 6)
#define ZZ_ATTR_EXPLICIT_RGB (1u << 7)
#define ZZ_ATTR_HYPERLINK (1u << 8)
#define ZZ_EVENT_DAMAGE_ALL (1u << 0)

/* One terminal cell, row-major in the plane returned by zz_viewport_cells.
 * Glyphs at or above (1u << 31) index the snapshot's grapheme table. */
typedef struct zz_cell {
    uint32_t glyph;
    uint16_t style;
    uint16_t flags;
} zz_cell;

typedef struct zz_style {
    uint32_t foreground;
    uint32_t background;
    uint32_t underline_color;
    uint16_t attributes;
    uint8_t underline_kind;
    uint8_t reserved;
} zz_style;

typedef struct zz_bytes {
    const uint8_t *ptr;
    size_t len;
} zz_bytes;

typedef enum zz_pane_kind {
    ZZ_PANE_PICKER = 0,
    ZZ_PANE_TERMINAL = 1,
    ZZ_PANE_BROWSER = 2,
    ZZ_PANE_AGENT = 3,
    ZZ_PANE_EDITOR = 4,
} zz_pane_kind;

typedef enum zz_key_code {
    ZZ_KEY_CHARACTER = 0,
    ZZ_KEY_BACKSPACE = 1,
    ZZ_KEY_ENTER = 2,
    ZZ_KEY_TAB = 3,
    ZZ_KEY_ESCAPE = 4,
    ZZ_KEY_DELETE = 5,
    ZZ_KEY_INSERT = 6,
    ZZ_KEY_HOME = 7,
    ZZ_KEY_END = 8,
    ZZ_KEY_PAGE_UP = 9,
    ZZ_KEY_PAGE_DOWN = 10,
    ZZ_KEY_ARROW_UP = 11,
    ZZ_KEY_ARROW_DOWN = 12,
    ZZ_KEY_ARROW_LEFT = 13,
    ZZ_KEY_ARROW_RIGHT = 14,
    ZZ_KEY_FUNCTION = 15,
    ZZ_KEY_UNIDENTIFIED = 16,
} zz_key_code;

typedef enum zz_key_action {
    ZZ_KEY_PRESS = 0,
    ZZ_KEY_REPEAT = 1,
    ZZ_KEY_RELEASE = 2,
} zz_key_action;

typedef struct zz_cursor {
    uint32_t color;
    uint16_t column;
    uint16_t row;
    uint8_t style;
    uint8_t visible;
    uint8_t blinking;
    uint8_t wide_tail;
} zz_cursor;

typedef enum zz_event_kind {
    ZZ_EVENT_HELLO = 0,
    ZZ_EVENT_ATTACHED = 1,
    ZZ_EVENT_SNAPSHOT_CHANGED = 2,
    ZZ_EVENT_VIEWPORT_CHANGED = 3,
    ZZ_EVENT_PANE_REMOVED = 4,
    ZZ_EVENT_STATUS_CHANGED = 5,
    ZZ_EVENT_DETACHED = 6,
    ZZ_EVENT_SERVER_STOPPING = 7,
    ZZ_EVENT_OTHER = 8,
    ZZ_EVENT_APPEARANCE_CHANGED = 9,
    ZZ_EVENT_DISCONNECTED = 10,
} zz_event_kind;

typedef struct zz_client_event {
    zz_event_kind kind;
    uint32_t flags;
    uint64_t pane;
    uint16_t row_start;
    uint16_t row_end;
} zz_client_event;

/* Connect to a daemon socket; NULL on failure. Free with zz_client_free. */
zz_client *zz_client_connect(const char *socket_path);
void zz_client_free(zz_client *client);

/* Readable whenever events are queued. Poll it, then drain
 * zz_client_next_event until it returns false. */
int zz_client_event_fd(const zz_client *client);
bool zz_client_next_event(zz_client *client, zz_client_event *out);

bool zz_client_attach(zz_client *client, const char *session);
bool zz_client_send_text(zz_client *client, uint64_t pane, const char *text);
bool zz_client_send_key(zz_client *client, uint64_t pane, uint32_t code,
                        uint32_t codepoint, uint8_t function, uint32_t action,
                        uint8_t modifiers, const char *text,
                        bool text_follows);
bool zz_client_execute(zz_client *client, const char *name,
                       const char *const *args, size_t args_len);
bool zz_client_resize_terminal(zz_client *client, uint64_t pane,
                               uint16_t columns, uint16_t rows,
                               uint32_t cell_width_px, uint32_t cell_height_px);
bool zz_client_scroll_lines(zz_client *client, uint64_t pane, int32_t lines);
bool zz_client_focus_terminal(zz_client *client, uint64_t pane, bool focused);

zz_mux_snapshot *zz_client_snapshot_acquire(const zz_client *client);
void zz_snapshot_release(zz_mux_snapshot *snapshot);
uint64_t zz_snapshot_generation(const zz_mux_snapshot *snapshot);
size_t zz_snapshot_session_count(const zz_mux_snapshot *snapshot);
uint64_t zz_snapshot_session_id(const zz_mux_snapshot *snapshot, size_t session);
zz_bytes zz_snapshot_session_name(const zz_mux_snapshot *snapshot, size_t session);
bool zz_snapshot_session_is_attached(const zz_mux_snapshot *snapshot,
                                     size_t session);
uint64_t zz_snapshot_session_active_window(const zz_mux_snapshot *snapshot,
                                           size_t session);
size_t zz_snapshot_session_pane_count(const zz_mux_snapshot *snapshot,
                                      size_t session);
uint64_t zz_snapshot_session_pane_id(const zz_mux_snapshot *snapshot,
                                     size_t session, size_t pane);
zz_bytes zz_snapshot_session_pane_title(const zz_mux_snapshot *snapshot,
                                        size_t session, size_t pane);
zz_pane_kind zz_snapshot_session_pane_kind(const zz_mux_snapshot *snapshot,
                                           size_t session, size_t pane);
bool zz_snapshot_session_pane_is_active(const zz_mux_snapshot *snapshot,
                                        size_t session, size_t pane);
bool zz_snapshot_session_pane_has_bell(const zz_mux_snapshot *snapshot,
                                       size_t session, size_t pane);

/* Writes up to capacity terminal pane ids from the attached session;
 * returns the total count. Empty until an attach lands. */
size_t zz_client_terminal_panes(const zz_client *client, uint64_t *out,
                                size_t capacity);

/* Caller-owned viewport snapshot; NULL when the pane holds none. Cheap to
 * acquire (shared immutable planes) and stable until released. */
zz_viewport *zz_client_viewport_acquire(const zz_client *client, uint64_t pane);
void zz_viewport_release(zz_viewport *viewport);
uint16_t zz_viewport_columns(const zz_viewport *viewport);
uint16_t zz_viewport_rows(const zz_viewport *viewport);
uint64_t zz_viewport_generation(const zz_viewport *viewport);
uint64_t zz_viewport_view_generation(const zz_viewport *viewport);
uint32_t zz_viewport_dictionary_generation(const zz_viewport *viewport);
uint32_t zz_viewport_foreground(const zz_viewport *viewport);
uint32_t zz_viewport_background(const zz_viewport *viewport);
const zz_cell *zz_viewport_cells(const zz_viewport *viewport);
const zz_style *zz_viewport_styles(const zz_viewport *viewport);
size_t zz_viewport_style_count(const zz_viewport *viewport);
const uint32_t *zz_viewport_grapheme_offsets(const zz_viewport *viewport);
size_t zz_viewport_grapheme_offset_count(const zz_viewport *viewport);
const uint8_t *zz_viewport_grapheme_bytes(const zz_viewport *viewport);
size_t zz_viewport_grapheme_byte_count(const zz_viewport *viewport);
bool zz_viewport_cursor(const zz_viewport *viewport, zz_cursor *out);

/* Decode one row as NUL-terminated UTF-8; returns bytes written. */
size_t zz_viewport_row_text(const zz_viewport *viewport, uint16_t row,
                            char *buf, size_t capacity);

#ifdef __cplusplus
}
#endif

#endif /* ZZ_CLIENT_H */
