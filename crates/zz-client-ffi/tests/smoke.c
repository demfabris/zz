/* The proof client: attach to a zz daemon through the C ABI alone, wait for
 * the fixture pane's content, type into it, and verify the echo — a complete
 * (if tiny) zz client with no Rust in sight. Exits 0 on success. */

#include <poll.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

#include "zz-client.h"

static const char READY[] = "zz-smoke-ready";
static const char TYPED[] = "hello-from-c";

static int bytes_equal(zz_bytes value, const char *expected) {
    size_t expected_len = strlen(expected);
    return value.len == expected_len &&
           memcmp(value.ptr, expected, expected_len) == 0;
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
    zz_client *client = zz_client_connect(argv[1]);
    if (client == NULL) {
        fprintf(stderr, "smoke: connect failed\n");
        return 1;
    }
    if (!zz_client_attach(client, "smoke")) {
        fprintf(stderr, "smoke: attach failed\n");
        return 1;
    }

    uint64_t panes[8];
    size_t pane_count = 0;
    for (int attempt = 0; attempt < 200 && pane_count == 0; attempt++) {
        zz_client_event event;
        while (zz_client_next_event(client, &event)) {
        }
        pane_count = zz_client_terminal_panes(client, panes, 8);
        if (pane_count == 0) {
            usleep(50 * 1000);
        }
    }
    if (pane_count == 0) {
        fprintf(stderr, "smoke: no terminal pane appeared\n");
        return 1;
    }
    uint64_t pane = panes[0];
    zz_mux_snapshot *snapshot = zz_client_snapshot_acquire(client);
    if (snapshot == NULL || zz_snapshot_session_count(snapshot) == 0) {
        fprintf(stderr, "smoke: no mux snapshot appeared\n");
        return 1;
    }
    size_t session_count = zz_snapshot_session_count(snapshot);
    int found_session = 0;
    for (size_t index = 0; index < zz_snapshot_session_count(snapshot); index++) {
        if (bytes_equal(zz_snapshot_session_name(snapshot, index), "smoke")) {
            found_session = zz_snapshot_session_is_attached(snapshot, index) &&
                            zz_snapshot_session_pane_count(snapshot, index) == 1 &&
                            zz_snapshot_session_pane_id(snapshot, index, 0) == pane &&
                            zz_snapshot_session_pane_kind(snapshot, index, 0) ==
                                ZZ_PANE_TERMINAL;
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
        fprintf(stderr, "smoke: recovery attach failed\n");
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

    zz_client_free(client);
    client = zz_client_connect(argv[1]);
    if (client == NULL || !zz_client_attach(client, "smoke")) {
        fprintf(stderr, "smoke: reconnect after free failed\n");
        return 1;
    }
    sleep(2);
    zz_client_free(client);
    printf("smoke: ok\n");
    return 0;
}
