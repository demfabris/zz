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
typedef struct zz_viewport zz_viewport;

/* One terminal cell, row-major in the plane returned by zz_viewport_cells.
 * Glyphs at or above (1u << 31) index the snapshot's grapheme table. */
typedef struct zz_cell {
    uint32_t glyph;
    uint16_t style;
    uint16_t flags;
} zz_cell;

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
} zz_event_kind;

typedef struct zz_client_event {
    zz_event_kind kind;
    uint64_t pane; /* zero when the kind carries no pane */
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
bool zz_client_execute(zz_client *client, const char *name,
                       const char *const *args, size_t args_len);
bool zz_client_resize_terminal(zz_client *client, uint64_t pane,
                               uint16_t columns, uint16_t rows,
                               uint32_t cell_width_px, uint32_t cell_height_px);

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
const zz_cell *zz_viewport_cells(const zz_viewport *viewport);

/* Decode one row as NUL-terminated UTF-8; returns bytes written. */
size_t zz_viewport_row_text(const zz_viewport *viewport, uint16_t row,
                            char *buf, size_t capacity);

#ifdef __cplusplus
}
#endif

#endif /* ZZ_CLIENT_H */
