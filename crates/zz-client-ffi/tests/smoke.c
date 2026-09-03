/* The proof client: attach to a zz daemon through the C ABI alone, wait for
 * the fixture pane's content, type into it, and verify the echo — a complete
 * (if tiny) zz client with no Rust in sight. Exits 0 on success. */

#include <poll.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

#include "zz-client.h"

static const char READY[] = "zz-smoke-ready";
static const char PREVIEW_READY[] = "zz-preview-ready";
static const char TYPED[] = "hello-from-c";
static const char PASTED[] = "pasted-from-c";
static const char REPLY_TEXT[] = "zz-reply-from-c";

static int bytes_equal(zz_bytes value, const char *expected) {
    size_t expected_len = strlen(expected);
    return value.len == expected_len &&
           memcmp(value.ptr, expected, expected_len) == 0;
}

static int bytes_same(zz_bytes left, zz_bytes right) {
    return left.len == right.len &&
           (left.len == 0 || memcmp(left.ptr, right.ptr, left.len) == 0);
}

static int viewport_contains(zz_client *client, uint64_t pane, const char *needle) {
    zz_viewport *viewport = zz_client_viewport_acquire(client, pane);
    if (viewport == NULL) {
        return 0;
    }
    char row_text[512];
    int found = 0;
    uint16_t rows = zz_viewport_rows(viewport);
    for (uint16_t row = 0; row < rows && !found; row++) {
        if (zz_viewport_row_text(viewport, row, row_text, sizeof row_text) > 0 &&
            strstr(row_text, needle) != NULL) {
            found = 1;
        }
    }
    zz_viewport_release(viewport);
    return found;
}

static int wait_for_text(zz_client *client, uint64_t pane, const char *needle) {
    struct pollfd wake = {.fd = zz_client_event_fd(client), .events = POLLIN};
    for (int attempt = 0; attempt < 400; attempt++) {
        zz_client_event event;
        while (zz_client_next_event(client, &event)) {
        }
        if (viewport_contains(client, pane, needle)) {
            return 1;
        }
        (void)poll(&wake, 1, 50);
    }
    return 0;
}

static int wait_for_attached_shape(zz_client *client, const char *excluded_name,
                                   size_t min_sessions, size_t min_panes,
                                   uint64_t *session_id,
                                   uint64_t *first_pane) {
    struct pollfd wake = {.fd = zz_client_event_fd(client), .events = POLLIN};
    for (int attempt = 0; attempt < 400; attempt++) {
        zz_client_event event;
        while (zz_client_next_event(client, &event)) {
        }
        zz_mux_snapshot *snapshot = zz_client_snapshot_acquire(client);
        if (snapshot != NULL &&
            zz_snapshot_session_count(snapshot) >= min_sessions) {
            for (size_t index = 0; index < zz_snapshot_session_count(snapshot);
                 index++) {
                if (zz_snapshot_session_is_attached(snapshot, index) &&
                    !bytes_equal(zz_snapshot_session_name(snapshot, index),
                                 excluded_name) &&
                    zz_snapshot_session_pane_count(snapshot, index) >=
                        min_panes) {
                    *session_id = zz_snapshot_session_id(snapshot, index);
                    *first_pane =
                        zz_snapshot_session_pane_id(snapshot, index, 0);
                    zz_snapshot_release(snapshot);
                    return 1;
                }
            }
        }
        zz_snapshot_release(snapshot);
        (void)poll(&wake, 1, 50);
    }
    return 0;
}

/* Drain replies until the one this request id owns turns up; the caller
 * releases it. Replies for other commands are freed on the way past. */
static zz_command_reply *wait_for_command_reply(zz_client *client,
                                                uint64_t request_id) {
    struct pollfd wake = {.fd = zz_client_event_fd(client), .events = POLLIN};
    for (int attempt = 0; attempt < 400; attempt++) {
        zz_client_event event;
        while (zz_client_next_event(client, &event)) {
        }
        zz_command_reply *reply = NULL;
        while ((reply = zz_client_command_reply_next(client)) != NULL) {
            if (zz_command_reply_request_id(reply) == request_id) {
                return reply;
            }
            zz_command_reply_release(reply);
        }
        (void)poll(&wake, 1, 50);
    }
    return NULL;
}

static int wait_for_detached_shape(zz_client *client, size_t session_count) {
    struct pollfd wake = {.fd = zz_client_event_fd(client), .events = POLLIN};
    for (int attempt = 0; attempt < 400; attempt++) {
        zz_client_event event;
        while (zz_client_next_event(client, &event)) {
        }
        zz_mux_snapshot *snapshot = zz_client_snapshot_acquire(client);
        int detached = snapshot != NULL &&
                       zz_snapshot_session_count(snapshot) == session_count;
        if (detached) {
            for (size_t index = 0; index < session_count; index++) {
                if (zz_snapshot_session_is_attached(snapshot, index)) {
                    detached = 0;
                    break;
                }
            }
        }
        zz_snapshot_release(snapshot);
        if (detached) {
            return 1;
        }
        (void)poll(&wake, 1, 50);
    }
    return 0;
}

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr, "usage: smoke <socket>\n");
        return 2;
    }
    char connect_error[512];
    zz_connect_failure connect_failure = ZZ_CONNECT_FAILURE_NONE;
    zz_client *invalid = zz_client_connect_endpoint_interactive(
        "", NULL, NULL, &connect_failure, connect_error, sizeof connect_error);
    if (invalid != NULL || connect_error[0] == '\0' ||
        connect_failure != ZZ_CONNECT_FAILURE_CONFIGURATION) {
        fprintf(stderr,
                "smoke: invalid endpoint did not report a configuration error\n");
        return 1;
    }
    zz_client *client = zz_client_connect_endpoint(
        argv[1], NULL, connect_error, sizeof connect_error);
    if (client == NULL) {
        fprintf(stderr, "smoke: connect failed: %s\n", connect_error);
        return 1;
    }
    struct pollfd initial_wake = {
        .fd = zz_client_event_fd(client),
        .events = POLLIN,
    };
    if (poll(&initial_wake, 1, 0) != 1 ||
        (initial_wake.revents & POLLIN) == 0) {
        fprintf(stderr, "smoke: initial hello did not wake the event fd\n");
        return 1;
    }
    zz_client_event initial_event;
    int saw_hello = 0;
    while (zz_client_next_event(client, &initial_event)) {
        if (initial_event.kind == ZZ_EVENT_HELLO) {
            saw_hello = 1;
        }
    }
    if (!saw_hello) {
        fprintf(stderr, "smoke: initial hello event was not queued\n");
        return 1;
    }
    if (!zz_client_attach(client, "smoke")) {
        fprintf(stderr, "smoke: attach request send failed\n");
        return 1;
    }

    uint64_t panes[8];
    size_t pane_count = 0;
    for (int attempt = 0; attempt < 200 && pane_count < 2; attempt++) {
        zz_client_event event;
        while (zz_client_next_event(client, &event)) {
        }
        pane_count = zz_client_terminal_panes(client, panes, 8);
        if (pane_count < 2) {
            usleep(50 * 1000);
        }
    }
    if (pane_count < 2) {
        fprintf(stderr, "smoke: both terminal windows did not appear\n");
        return 1;
    }
    if (!zz_client_set_terminal_preview(client, true)) {
        fprintf(stderr, "smoke: terminal preview enable failed\n");
        return 1;
    }
    uint64_t pane = 0;
    uint64_t preview_pane = 0;
    zz_mux_snapshot *snapshot = zz_client_snapshot_acquire(client);
    if (snapshot == NULL || zz_snapshot_session_count(snapshot) == 0) {
        fprintf(stderr, "smoke: no mux snapshot appeared\n");
        return 1;
    }
    if (!zz_client_set_focused(client, true)) {
        fprintf(stderr, "smoke: client focus after attach failed\n");
        return 1;
    }
    size_t session_count = zz_snapshot_session_count(snapshot);
    int found_session = 0;
    for (size_t index = 0; index < zz_snapshot_session_count(snapshot); index++) {
        if (bytes_equal(zz_snapshot_session_name(snapshot, index), "smoke")) {
            int session_shape =
                zz_snapshot_session_is_attached(snapshot, index) &&
                zz_snapshot_session_pane_count(snapshot, index) == 1 &&
                zz_snapshot_session_window_count(snapshot, index) == 2;
            int current_windows = 0;
            int inactive_windows = 0;
            for (size_t window = 0; window < 2 && session_shape; window++) {
                zz_pane_rect rect = {0};
                uint64_t zoomed = 0;
                uint64_t window_pane =
                    zz_snapshot_session_window_pane_id(snapshot, index, window, 0);
                session_shape =
                    zz_snapshot_session_window_name(snapshot, index, window).len > 0 &&
                    zz_snapshot_session_window_active_pane(snapshot, index, window) ==
                        window_pane &&
                    zz_snapshot_session_window_pane_count(snapshot, index, window) == 1 &&
                    zz_snapshot_session_window_pane_kind(snapshot, index, window, 0) ==
                        ZZ_PANE_TERMINAL &&
                    zz_snapshot_session_window_pane_is_active(snapshot, index, window, 0) &&
                    zz_snapshot_session_window_pane_rect(snapshot, index, window, 0, &rect) &&
                    rect.x == 0.0f && rect.y == 0.0f && rect.width == 1.0f &&
                    rect.height == 1.0f &&
                    !zz_snapshot_session_window_zoomed_pane(snapshot, index, window,
                                                            &zoomed);
                if (zz_snapshot_session_window_is_current(snapshot, index, window)) {
                    current_windows++;
                    pane = window_pane;
                    session_shape =
                        session_shape &&
                        zz_snapshot_session_window_id(snapshot, index, window) ==
                            zz_snapshot_session_active_window(snapshot, index) &&
                        zz_snapshot_session_pane_id(snapshot, index, 0) == window_pane &&
                        zz_snapshot_session_pane_kind(snapshot, index, 0) ==
                            ZZ_PANE_TERMINAL &&
                        bytes_same(
                            zz_snapshot_session_window_pane_title(snapshot, index, window, 0),
                            zz_snapshot_session_pane_title(snapshot, index, 0)) &&
                        zz_snapshot_session_window_pane_has_bell(snapshot, index, window, 0) ==
                            zz_snapshot_session_pane_has_bell(snapshot, index, 0);
                } else {
                    inactive_windows++;
                    preview_pane = window_pane;
                }
            }
            found_session = session_shape && current_windows == 1 && inactive_windows == 1;
        }
    }
    zz_snapshot_release(snapshot);
    if (!found_session) {
        fprintf(stderr, "smoke: attached session metadata is incomplete\n");
        return 1;
    }
    if (!zz_client_resize_terminal(client, pane, 80, 24, 8, 16)) {
        fprintf(stderr, "smoke: resize failed\n");
        return 1;
    }
    if (!wait_for_text(client, pane, READY)) {
        fprintf(stderr, "smoke: fixture text never arrived\n");
        return 1;
    }
    if (!wait_for_text(client, preview_pane, PREVIEW_READY)) {
        fprintf(stderr, "smoke: inactive-window preview never arrived\n");
        return 1;
    }
    zz_viewport *viewport = zz_client_viewport_acquire(client, pane);
    int viewport_complete =
        viewport != NULL && zz_viewport_cells(viewport) != NULL &&
        zz_viewport_styles(viewport) != NULL &&
        zz_viewport_style_count(viewport) > 0 &&
        zz_viewport_grapheme_offsets(viewport) != NULL &&
        zz_viewport_grapheme_offset_count(viewport) > 0;
    zz_viewport_release(viewport);
    if (!viewport_complete) {
        fprintf(stderr, "smoke: graphical viewport contract is incomplete\n");
        return 1;
    }
    if (!zz_client_terminal_selection(client, pane, 0, 0, 0, 1, false) ||
        !zz_client_terminal_selection(client, pane, 1, 1, 0, 1, false) ||
        !zz_client_terminal_selection(client, pane, 2, 1, 0, 1, false) ||
        !zz_client_copy_selection(client, pane, 1)) {
        fprintf(stderr, "smoke: terminal selection contract failed\n");
        return 1;
    }
    zz_agent_state *agent = zz_client_agent_state_acquire(client, pane);
    if (agent != NULL) {
        (void)zz_agent_state_phase(agent);
        (void)zz_agent_attention_status(agent);
        zz_agent_state_release(agent);
    }
    zz_clipboard *clipboard = zz_client_clipboard_next(client);
    if (clipboard != NULL) {
        (void)zz_clipboard_text(clipboard);
        zz_clipboard_release(clipboard);
    }

    if (!zz_client_send_text(client, pane, TYPED)) {
        fprintf(stderr, "smoke: send failed\n");
        return 1;
    }
    if (!zz_client_send_key(client, pane, ZZ_KEY_ENTER, 0, 0, ZZ_KEY_PRESS,
                            0, NULL, false)) {
        fprintf(stderr, "smoke: raw enter failed\n");
        return 1;
    }
    if (!wait_for_text(client, pane, TYPED)) {
        fprintf(stderr, "smoke: typed text never echoed\n");
        return 1;
    }
    if (!zz_client_paste(client, pane, PASTED)) {
        fprintf(stderr, "smoke: paste failed\n");
        return 1;
    }
    if (!wait_for_text(client, pane, PASTED)) {
        fprintf(stderr, "smoke: pasted text never echoed\n");
        return 1;
    }

    if (!zz_client_execute(client, "new-session", NULL, 0)) {
        fprintf(stderr, "smoke: new-session send failed\n");
        return 1;
    }
    uint64_t created_session = 0;
    uint64_t created_pane = 0;
    if (!wait_for_attached_shape(client, "smoke", session_count + 1, 1,
                                 &created_session, &created_pane)) {
        fprintf(stderr, "smoke: new session was not created and attached\n");
        return 1;
    }
    char target[32];
    snprintf(target, sizeof target, "%%%llu",
             (unsigned long long)created_pane);
    const char *split[] = {"-t", target};
    if (!zz_client_execute(client, "split-window", split, 2)) {
        fprintf(stderr, "smoke: split-window send failed\n");
        return 1;
    }
    if (!wait_for_attached_shape(client, "smoke", session_count + 1, 2,
                                 &created_session, &created_pane)) {
        fprintf(stderr, "smoke: new pane did not appear\n");
        return 1;
    }

    char session_target[32];
    snprintf(session_target, sizeof session_target, "$%llu",
             (unsigned long long)created_session);
    const char *kill[] = {"-t", session_target};
    if (!zz_client_execute(client, "kill-session", kill, 2)) {
        fprintf(stderr, "smoke: kill-session send failed\n");
        return 1;
    }
    if (!wait_for_detached_shape(client, session_count)) {
        fprintf(stderr, "smoke: killed session did not detach the client\n");
        return 1;
    }
    if (!zz_client_attach(client, "smoke")) {
        fprintf(stderr, "smoke: recovery attach request send failed\n");
        return 1;
    }
    uint64_t recovered_session = 0;
    uint64_t recovered_pane = 0;
    if (!wait_for_attached_shape(client, "", session_count, 1,
                                 &recovered_session, &recovered_pane) ||
        recovered_pane != pane || !wait_for_text(client, recovered_pane, READY)) {
        fprintf(stderr, "smoke: surviving session viewport did not recover\n");
        return 1;
    }

    const char *printed[] = {"-p", "-l", REPLY_TEXT};
    uint64_t printed_request =
        zz_client_execute_request(client, "display-message", printed, 3);
    if (printed_request == 0) {
        fprintf(stderr, "smoke: printing command send failed\n");
        return 1;
    }
    zz_command_reply *reply = wait_for_command_reply(client, printed_request);
    if (reply == NULL) {
        fprintf(stderr, "smoke: printing command never replied\n");
        return 1;
    }
    int printed_ok = zz_command_reply_ok(reply) &&
                     zz_command_reply_exit_code(reply) == 0 &&
                     zz_command_reply_error(reply).len == 0 &&
                     bytes_equal(zz_command_reply_output(reply), REPLY_TEXT);
    zz_command_reply_release(reply);
    if (!printed_ok) {
        fprintf(stderr, "smoke: printed command reply text is wrong\n");
        return 1;
    }
    if (!zz_client_cancel_command_output(client)) {
        fprintf(stderr, "smoke: command output cancel failed\n");
        return 1;
    }

    uint64_t rejected_request =
        zz_client_execute_request(client, "show-last-output", NULL, 0);
    if (rejected_request == 0) {
        fprintf(stderr, "smoke: show-last-output send failed\n");
        return 1;
    }
    reply = wait_for_command_reply(client, rejected_request);
    if (reply == NULL) {
        fprintf(stderr, "smoke: show-last-output never replied\n");
        return 1;
    }
    int rejection_reported = !zz_command_reply_ok(reply) &&
                             zz_command_reply_exit_code(reply) == 1 &&
                             zz_command_reply_error(reply).len > 0;
    zz_command_reply_release(reply);
    if (!rejection_reported) {
        fprintf(stderr, "smoke: rejected command carried no error text\n");
        return 1;
    }

    if (!zz_client_set_focused(client, false)) {
        fprintf(stderr, "smoke: client blur failed\n");
        return 1;
    }
    if (!zz_client_set_terminal_preview(client, false)) {
        fprintf(stderr, "smoke: terminal preview disable failed\n");
        return 1;
    }
    zz_client_free(client);
    client = zz_client_connect(argv[1]);
    if (client == NULL) {
        fprintf(stderr, "smoke: reconnect after free failed\n");
        return 1;
    }
    if (!zz_client_attach(client, "smoke")) {
        fprintf(stderr, "smoke: reconnect attach request send failed\n");
        return 1;
    }
    sleep(2);
    zz_client_free(client);
    printf("smoke: ok\n");
    return 0;
}
