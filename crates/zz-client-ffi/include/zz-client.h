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
typedef struct zz_agent_state zz_agent_state;
typedef struct zz_clipboard zz_clipboard;

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
#define ZZ_EVENT_AGENT_REQUEST (1u << 1)
#define ZZ_EVENT_AGENT_DONE (1u << 2)
#define ZZ_EVENT_AGENT_FAILED (1u << 3)

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

typedef struct zz_pane_rect {
    float x;
    float y;
    float width;
    float height;
} zz_pane_rect;

typedef enum zz_connect_failure {
    ZZ_CONNECT_FAILURE_NONE = 0,
    ZZ_CONNECT_FAILURE_RETRYABLE = 1,
    ZZ_CONNECT_FAILURE_AUTHENTICATION = 2,
    ZZ_CONNECT_FAILURE_HOST_KEY = 3,
    ZZ_CONNECT_FAILURE_CONFIGURATION = 4,
    ZZ_CONNECT_FAILURE_INCOMPATIBLE = 5,
} zz_connect_failure;

typedef enum zz_ssh_prompt_kind {
    ZZ_SSH_PROMPT_SECRET = 0,
    ZZ_SSH_PROMPT_HOST_KEY = 1,
    ZZ_SSH_PROMPT_CONFIRMATION = 2,
} zz_ssh_prompt_kind;

typedef enum zz_ssh_prompt_reply {
    ZZ_SSH_PROMPT_CANCEL = 0,
    ZZ_SSH_PROMPT_ANSWER = 1,
    ZZ_SSH_PROMPT_TRUST_ONCE = 2,
    ZZ_SSH_PROMPT_TRUST_AND_SAVE = 3,
} zz_ssh_prompt_reply;

typedef struct zz_ssh_prompt {
    zz_ssh_prompt_kind kind;
    zz_bytes title;
    zz_bytes message;
    bool echo;
} zz_ssh_prompt;

typedef zz_ssh_prompt_reply (*zz_ssh_prompt_callback)(
    void *context, const zz_ssh_prompt *prompt, char *response,
    size_t response_capacity);

typedef enum zz_pane_kind {
    ZZ_PANE_PICKER = 0,
    ZZ_PANE_TERMINAL = 1,
    ZZ_PANE_BROWSER = 2,
    ZZ_PANE_AGENT = 3,
    ZZ_PANE_EDITOR = 4,
} zz_pane_kind;

typedef enum zz_agent_phase {
    ZZ_AGENT_STARTING = 0,
    ZZ_AGENT_READY = 1,
    ZZ_AGENT_RUNNING = 2,
    ZZ_AGENT_AWAITING_PERMISSION = 3,
    ZZ_AGENT_FAILED = 4,
} zz_agent_phase;

typedef enum zz_agent_attention {
    ZZ_AGENT_IDLE = 0,
    ZZ_AGENT_WORKING = 1,
    ZZ_AGENT_NEEDS_INPUT = 2,
    ZZ_AGENT_ATTENTION_FAILED = 3,
} zz_agent_attention;

typedef enum zz_agent_permission_kind {
    ZZ_AGENT_PERMISSION_UNKNOWN = 0,
    ZZ_AGENT_PERMISSION_ALLOW_ONCE = 1,
    ZZ_AGENT_PERMISSION_ALLOW_ALWAYS = 2,
    ZZ_AGENT_PERMISSION_REJECT_ONCE = 3,
    ZZ_AGENT_PERMISSION_REJECT_ALWAYS = 4,
} zz_agent_permission_kind;

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
    ZZ_EVENT_AGENT_STATE_CHANGED = 11,
    ZZ_EVENT_CLIPBOARD = 12,
    ZZ_EVENT_PREFIX_ARMED = 13,
    ZZ_EVENT_KEY_TABLES_CHANGED = 14,
    ZZ_EVENT_COMMAND_PROMPT_CHANGED = 15,
    ZZ_EVENT_CHOOSE_BUFFER_CHANGED = 16,
    ZZ_EVENT_DISPLAY_PANES_CHANGED = 17,
    ZZ_EVENT_AGENT_UPDATES = 18,
    ZZ_EVENT_AGENT_LAGGED = 19,
    ZZ_EVENT_AGENT_SESSIONS = 20,
    ZZ_EVENT_COMMAND_REPLY = 21,
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
zz_client *zz_client_connect_endpoint(const char *endpoint,
                                      const char *password, char *error,
                                      size_t error_capacity);
zz_client *zz_client_connect_endpoint_interactive(
    const char *endpoint, zz_ssh_prompt_callback callback, void *context,
    zz_connect_failure *failure, char *error, size_t error_capacity);
size_t zz_client_ssh_public_key(char *buf, size_t capacity);
void zz_client_free(zz_client *client);

/* Readable whenever events are queued. Poll it, then drain
 * zz_client_next_event until it returns false. */
int zz_client_event_fd(const zz_client *client);
bool zz_client_next_event(zz_client *client, zz_client_event *out);

bool zz_client_attach(zz_client *client, const char *session);
bool zz_client_set_terminal_preview(zz_client *client, bool enabled);
bool zz_client_send_text(zz_client *client, uint64_t pane, const char *text);
bool zz_client_send_key(zz_client *client, uint64_t pane, uint32_t code,
                        uint32_t codepoint, uint8_t function, uint32_t action,
                        uint8_t modifiers, const char *text,
                        bool text_follows);
/* Execute a tmux-style command and throw the reply away. */
bool zz_client_execute(zz_client *client, const char *name,
                       const char *const *args, size_t args_len);
/* Execute a tmux-style command and return the request id its reply carries;
 * 0 when the request could not be sent. The daemon answers every command, so
 * a ZZ_EVENT_COMMAND_REPLY follows: pop it with
 * zz_client_command_reply_next and match the request id. */
uint64_t zz_client_execute_request(zz_client *client, const char *name,
                                   const char *const *args, size_t args_len);
bool zz_client_resize_terminal(zz_client *client, uint64_t pane,
                               uint16_t columns, uint16_t rows,
                               uint32_t cell_width_px, uint32_t cell_height_px);
bool zz_client_scroll_lines(zz_client *client, uint64_t pane, int32_t lines);
bool zz_client_terminal_selection(zz_client *client, uint64_t pane,
                                  uint32_t phase, uint16_t column,
                                  uint16_t row, uint8_t click_count,
                                  bool rectangle);
bool zz_client_copy_selection(zz_client *client, uint64_t pane,
                              uint64_t request_id);
bool zz_client_set_focused(zz_client *client, bool focused);
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
size_t zz_snapshot_session_window_count(const zz_mux_snapshot *snapshot,
                                        size_t session);
uint64_t zz_snapshot_session_window_id(const zz_mux_snapshot *snapshot,
                                       size_t session, size_t window);
uint32_t zz_snapshot_session_window_index(const zz_mux_snapshot *snapshot,
                                          size_t session, size_t window);
zz_bytes zz_snapshot_session_window_name(const zz_mux_snapshot *snapshot,
                                         size_t session, size_t window);
bool zz_snapshot_session_window_is_current(const zz_mux_snapshot *snapshot,
                                           size_t session, size_t window);
uint64_t zz_snapshot_session_window_active_pane(
    const zz_mux_snapshot *snapshot, size_t session, size_t window);
bool zz_snapshot_session_window_zoomed_pane(const zz_mux_snapshot *snapshot,
                                            size_t session, size_t window,
                                            uint64_t *out);
size_t zz_snapshot_session_window_pane_count(const zz_mux_snapshot *snapshot,
                                             size_t session, size_t window);
uint64_t zz_snapshot_session_window_pane_id(const zz_mux_snapshot *snapshot,
                                            size_t session, size_t window,
                                            size_t pane);
zz_bytes zz_snapshot_session_window_pane_title(const zz_mux_snapshot *snapshot,
                                               size_t session, size_t window,
                                               size_t pane);
zz_pane_kind zz_snapshot_session_window_pane_kind(
    const zz_mux_snapshot *snapshot, size_t session, size_t window, size_t pane);
bool zz_snapshot_session_window_pane_is_active(
    const zz_mux_snapshot *snapshot, size_t session, size_t window, size_t pane);
bool zz_snapshot_session_window_pane_has_bell(
    const zz_mux_snapshot *snapshot, size_t session, size_t window, size_t pane);
bool zz_snapshot_session_window_pane_rect(const zz_mux_snapshot *snapshot,
                                          size_t session, size_t window,
                                          size_t pane, zz_pane_rect *out);
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

typedef struct zz_prefix_snapshot zz_prefix_snapshot;

/* Owned copy of the prefix arming plus the published `prefix` table's
 * bindings. Acquire per refresh, read the accessors below, then release.
 * String results borrow the snapshot and die with it. */
zz_prefix_snapshot *zz_prefix_snapshot_acquire(const zz_client *client);
void zz_prefix_snapshot_release(zz_prefix_snapshot *snapshot);
bool zz_prefix_snapshot_armed(const zz_prefix_snapshot *snapshot);
size_t zz_prefix_binding_count(const zz_prefix_snapshot *snapshot);
zz_bytes zz_prefix_binding_key(const zz_prefix_snapshot *snapshot,
                               size_t binding);
bool zz_prefix_binding_repeat(const zz_prefix_snapshot *snapshot,
                              size_t binding);
zz_bytes zz_prefix_binding_note(const zz_prefix_snapshot *snapshot,
                                size_t binding);
/* First bound command as one line (`split-window -h`); empty when unbound. */
zz_bytes zz_prefix_binding_summary(const zz_prefix_snapshot *snapshot,
                                   size_t binding);

/* Writes up to capacity terminal pane ids from the attached session;
 * returns the total count. Empty until an attach lands. */
size_t zz_client_terminal_panes(const zz_client *client, uint64_t *out,
                                size_t capacity);

zz_agent_state *zz_client_agent_state_acquire(const zz_client *client,
                                              uint64_t pane);
void zz_agent_state_release(zz_agent_state *state);
zz_agent_phase zz_agent_state_phase(const zz_agent_state *state);
zz_agent_attention zz_agent_attention_status(const zz_agent_state *state);
uint32_t zz_agent_queued_prompts(const zz_agent_state *state);
zz_bytes zz_agent_session_id(const zz_agent_state *state);
zz_bytes zz_agent_title(const zz_agent_state *state);
zz_bytes zz_agent_error(const zz_agent_state *state);
bool zz_agent_has_permission(const zz_agent_state *state);
uint64_t zz_agent_permission_request_id(const zz_agent_state *state);
zz_bytes zz_agent_permission_payload(const zz_agent_state *state);
zz_bytes zz_agent_permission_title(const zz_agent_state *state);
size_t zz_agent_permission_option_count(const zz_agent_state *state);
zz_bytes zz_agent_permission_option_id(const zz_agent_state *state,
                                       size_t option);
zz_bytes zz_agent_permission_option_name(const zz_agent_state *state,
                                         size_t option);
zz_agent_permission_kind zz_agent_permission_option_kind(
    const zz_agent_state *state, size_t option);
bool zz_agent_has_git(const zz_agent_state *state);
zz_bytes zz_agent_git_branch(const zz_agent_state *state);
uint32_t zz_agent_git_changed_files(const zz_agent_state *state);
uint32_t zz_agent_git_additions(const zz_agent_state *state);
uint32_t zz_agent_git_deletions(const zz_agent_state *state);
bool zz_client_agent_respond_permission(zz_client *client, uint64_t pane,
                                        uint64_t request_id,
                                        const char *option_id);
bool zz_client_agent_cancel(zz_client *client, uint64_t pane);

/* One coalesced agent transcript batch. Pop the oldest with
 * zz_client_agent_updates_next (NULL when none is queued), read it with the
 * accessors below, then free it with zz_agent_updates_release. Each item is
 * one daemon JSON stream item; zz_agent_updates_first_seq numbers the first.
 * Item bytes are borrowed from the batch and die with it. */
typedef struct zz_agent_updates zz_agent_updates;

zz_agent_updates *zz_client_agent_updates_next(zz_client *client);
void zz_agent_updates_release(zz_agent_updates *updates);
uint64_t zz_agent_updates_pane(const zz_agent_updates *updates);
uint64_t zz_agent_updates_first_seq(const zz_agent_updates *updates);
size_t zz_agent_updates_item_count(const zz_agent_updates *updates);
zz_bytes zz_agent_updates_item_bytes(const zz_agent_updates *updates,
                                     size_t index);

/* Pop the oldest agent-lane overflow notice into the out pointers. The daemon
 * cleared the pane's lane from next_seq; answer with zz_client_agent_replay
 * from the shell's cursor. */
bool zz_client_agent_lagged_next(zz_client *client, uint64_t *pane_out,
                                 uint64_t *next_seq_out);

/* Ask the daemon to replay a pane's agent stream from from_seq, inclusively,
 * then tail it. Send on a journal gap, a lane overflow, and when a pane's
 * view goes live without a cursor. */
bool zz_client_agent_replay(zz_client *client, uint64_t pane,
                            uint64_t from_seq);

/* One agent session-list reply. Pop the oldest with
 * zz_client_agent_sessions_next (NULL when none is queued), read it with the
 * accessors below, then free it with zz_agent_sessions_release. The result is
 * the daemon's JSON reply: a sessionsListed payload on success, a
 * sessionListFailed one after a rejected list request. Result bytes are
 * borrowed from the reply and die with it. */
typedef struct zz_agent_sessions zz_agent_sessions;

zz_agent_sessions *zz_client_agent_sessions_next(zz_client *client);
void zz_agent_sessions_release(zz_agent_sessions *reply);
uint64_t zz_agent_sessions_pane(const zz_agent_sessions *reply);
uint64_t zz_agent_sessions_request_id(const zz_agent_sessions *reply);
zz_bytes zz_agent_sessions_result(const zz_agent_sessions *reply);

/* The pane's raw session-config JSON blob: an array of ACP
 * SessionConfigOption values (model, thoughtLevel, and mode categories drive
 * the pickers). Empty when the adapter published none. */
zz_bytes zz_agent_config_options(const zz_agent_state *state);

/* The pane's raw legacy session-mode JSON blob (SessionModeState), used only
 * when the adapter publishes no config options. Empty when absent. */
zz_bytes zz_agent_modes(const zz_agent_state *state);

/* Set one session config option (model, effort, permission mode) by id. */
bool zz_client_agent_set_config_option(zz_client *client, uint64_t pane,
                                       const char *option_id,
                                       const char *value);

/* Set the pane's legacy session mode by id. Adapters with config options
 * ignore this; it exists for adapters that only publish modes. */
bool zz_client_agent_set_mode(zz_client *client, uint64_t pane,
                              const char *mode_id);

/* Ask the daemon to list the pane's agent sessions across every project. The
 * answer arrives as ZZ_EVENT_AGENT_SESSIONS. */
bool zz_client_agent_list_sessions(zz_client *client, uint64_t pane);

/* Start a new agent session in the pane with cwd as its working directory.
 * The path must be absolute. */
bool zz_client_agent_new_session(zz_client *client, uint64_t pane,
                                 const char *cwd);

/* Switch the pane to a listed agent session. additional_directories_json is a
 * JSON array of absolute paths and may be NULL for none. */
bool zz_client_agent_switch_session(zz_client *client, uint64_t pane,
                                    const char *session_id, const char *cwd,
                                    const char *additional_directories_json);

/* Delete a listed agent session by id. */
bool zz_client_agent_delete_session(zz_client *client, uint64_t pane,
                                    const char *session_id);

zz_clipboard *zz_client_clipboard_next(zz_client *client);
void zz_clipboard_release(zz_clipboard *clipboard);
uint64_t zz_clipboard_pane(const zz_clipboard *clipboard);
uint64_t zz_clipboard_request_id(const zz_clipboard *clipboard);
zz_bytes zz_clipboard_text(const zz_clipboard *clipboard);

/* One executed command's answer. Pop the oldest with
 * zz_client_command_reply_next (NULL when none is queued), read it with the
 * accessors below, then free it with zz_command_reply_release. Output is the
 * text the verb prints (show-last-output, display-message -p, list-sessions,
 * ...); error carries the rendered server error and is empty on success.
 * Both borrow the reply and die with it. The queue keeps the 64 newest
 * unread replies, so drain it whenever you care about one. */
typedef struct zz_command_reply zz_command_reply;

zz_command_reply *zz_client_command_reply_next(zz_client *client);
void zz_command_reply_release(zz_command_reply *reply);
uint64_t zz_command_reply_request_id(const zz_command_reply *reply);
bool zz_command_reply_ok(const zz_command_reply *reply);
uint8_t zz_command_reply_exit_code(const zz_command_reply *reply);
zz_bytes zz_command_reply_output(const zz_command_reply *reply);
zz_bytes zz_command_reply_error(const zz_command_reply *reply);

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
