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
    if (!zz_client_resize_terminal(client, pane, 80, 24, 8, 16)) {
        fprintf(stderr, "smoke: resize failed\n");
        return 1;
    }
    if (!wait_for_text(client, pane, READY)) {
        fprintf(stderr, "smoke: fixture text never arrived\n");
        return 1;
    }

    if (!zz_client_send_text(client, pane, TYPED)) {
        fprintf(stderr, "smoke: send failed\n");
        return 1;
    }
    const char *enter[] = {"-t", NULL, "Enter"};
    char target[32];
    snprintf(target, sizeof target, "%%%llu", (unsigned long long)pane);
    enter[1] = target;
    if (!zz_client_execute(client, "send-keys", enter, 3)) {
        fprintf(stderr, "smoke: send-keys failed\n");
        return 1;
    }
    if (!wait_for_text(client, pane, TYPED)) {
        fprintf(stderr, "smoke: typed text never echoed\n");
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
