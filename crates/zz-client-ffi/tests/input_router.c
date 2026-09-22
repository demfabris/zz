#include <assert.h>
#include <stdio.h>
#include <string.h>

#include "zz-client.h"

static zz_input_owner pane_owner(uint64_t pane, zz_surface_kind surface) {
    return (zz_input_owner){ZZ_OWNER_PANE, pane, surface};
}

static void expect_bytes(zz_bytes actual, const char *expected) {
    size_t len = expected == NULL ? 0 : strlen(expected);
    assert(actual.len == len);
    assert(len == 0 || memcmp(actual.ptr, expected, len) == 0);
}

static void expect_owner(zz_input_owner actual, zz_input_owner expected) {
    assert(actual.kind == expected.kind);
    assert(actual.pane == expected.pane);
    assert(actual.surface == expected.surface);
}

static void expect_empty(zz_input_router *router) {
    zz_input_effect effect;
    assert(!zz_input_router_next_effect(router, &effect));
}

static void expect_focus(zz_input_router *router, zz_input_owner owner) {
    zz_input_effect effect;
    assert(zz_input_router_next_effect(router, &effect));
    assert(effect.kind == ZZ_EFFECT_REQUEST_FOCUS);
    expect_owner(effect.owner, owner);
    expect_empty(router);
}

static void expect_chrome(zz_input_router *router, const char *action) {
    zz_input_effect effect;
    assert(zz_input_router_next_effect(router, &effect));
    assert(effect.kind == ZZ_EFFECT_CHROME);
    expect_bytes(effect.action, action);
    expect_empty(router);
}

static void expect_forward(zz_input_router *router, uint64_t pane,
                           uint32_t code, uint32_t codepoint, uint8_t function,
                           uint32_t action, uint8_t modifiers, const char *text) {
    zz_input_effect effect;
    assert(zz_input_router_next_effect(router, &effect));
    assert(effect.kind == ZZ_EFFECT_FORWARD_KEY);
    assert(effect.pane == pane);
    assert(effect.key.code == code);
    assert(effect.key.codepoint == codepoint);
    assert(effect.key.function == function);
    assert(effect.key.action == action);
    assert(effect.key.modifiers == modifiers);
    expect_bytes(effect.key.text, text);
    expect_empty(router);
}

static zz_input_router *new_router(void) {
    zz_chrome_keymap *keymap = zz_chrome_keymap_new();
    assert(keymap != NULL);
    zz_input_router *router = zz_input_router_new(keymap);
    assert(router != NULL);
    return router;
}

static void activate(zz_input_router *router, zz_input_owner pane) {
    zz_input_router_event(router, ZZ_INPUT_ACTIVATE_PANE, pane);
    expect_owner(zz_input_router_owner(router), pane);
    expect_focus(router, pane);
}

static void pane_keys_stay_native(void) {
    zz_input_router *router = new_router();
    activate(router, pane_owner(17, ZZ_SURFACE_TERMINAL));
    assert(zz_input_router_key(router, ZZ_KEY_ENTER, 0, 0, 0, ZZ_KEY_PRESS,
                               0, NULL, false, false) == 0);
    expect_empty(router);
    assert(zz_input_router_key(router, ZZ_KEY_CHARACTER, 'x', 0, 0, ZZ_KEY_PRESS,
                               0, "x", false, false) == 0);
    expect_empty(router);
    zz_input_router_free(router);
}

static void sidebar_then_pending_pane(void) {
    zz_input_router *router = new_router();
    zz_input_owner sidebar = {ZZ_OWNER_SIDEBAR, 0, ZZ_SURFACE_TERMINAL};
    zz_input_router_event(router, ZZ_INPUT_FOCUS_SIDEBAR, sidebar);
    expect_owner(zz_input_router_owner(router), sidebar);
    expect_focus(router, sidebar);
    assert(zz_input_router_key(router, ZZ_KEY_ENTER, 0, 0, 0, ZZ_KEY_PRESS,
                               0, NULL, false, false) == 1);
    expect_chrome(router, "sidebar-confirm");
    activate(router, pane_owner(17, ZZ_SURFACE_TERMINAL));
    assert(zz_input_router_key(router, ZZ_KEY_ENTER, 0, 0, 0, ZZ_KEY_PRESS,
                               0, NULL, false, false) == 0);
    expect_empty(router);
    zz_input_router_free(router);
}

static void prefix_pairing_and_payloads(void) {
    zz_input_router *router = new_router();
    activate(router, pane_owner(17, ZZ_SURFACE_TERMINAL));
    char text[] = "prefix-\xc3\xa9";
    assert(zz_input_router_key(router, ZZ_KEY_CHARACTER, 'x', 0, 0, ZZ_KEY_PRESS,
                               6, text, true, true) == 1);
    text[0] = 'X';
    expect_forward(router, 17, ZZ_KEY_CHARACTER, 'x', 0, ZZ_KEY_PRESS,
                   6, "prefix-\xc3\xa9");
    assert(zz_input_router_key(router, ZZ_KEY_CHARACTER, 'x', 0, 0, ZZ_KEY_REPEAT,
                               6, "repeat", true, true) == 1);
    expect_empty(router);
    activate(router, pane_owner(23, ZZ_SURFACE_BROWSER));
    assert(zz_input_router_key(router, ZZ_KEY_CHARACTER, 'x', 0, 0, ZZ_KEY_RELEASE,
                               0, "release", false, false) == 1);
    expect_forward(router, 17, ZZ_KEY_CHARACTER, 'x', 0, ZZ_KEY_RELEASE,
                   0, "release");
    assert(zz_input_router_key(router, ZZ_KEY_CHARACTER, 'z', 0, 0, ZZ_KEY_RELEASE,
                               2, NULL, false, false) == 0);
    expect_empty(router);
    assert(zz_input_router_key(router, ZZ_KEY_FUNCTION, 0, 0, 12, ZZ_KEY_PRESS,
                               2, NULL, true, true) == 1);
    expect_forward(router, 23, ZZ_KEY_FUNCTION, 0, 12, ZZ_KEY_PRESS, 2, NULL);
    assert(zz_input_router_key(router, ZZ_KEY_FUNCTION, 0, 0, 12, ZZ_KEY_RELEASE,
                               0, NULL, false, false) == 1);
    expect_forward(router, 23, ZZ_KEY_FUNCTION, 0, 12, ZZ_KEY_RELEASE, 0, NULL);
    zz_input_router_free(router);
}

static void ui_precedes_prefix_and_owner(void) {
    zz_chrome_keymap *keymap = zz_chrome_keymap_new();
    assert(keymap != NULL);
    assert(zz_chrome_keymap_bind(keymap, "ui", "C-b", "open-command-palette"));
    assert(zz_chrome_keymap_bind(keymap, "ui", "D-1", "select-window-1"));
    assert(zz_chrome_keymap_bind(keymap, "browser", "D-1", "browser-select-tab-1"));
    zz_input_router *router = zz_input_router_new(keymap);
    assert(router != NULL);
    activate(router, pane_owner(17, ZZ_SURFACE_BROWSER));
    assert(zz_input_router_key(router, ZZ_KEY_CHARACTER, 'b', 0, 0, ZZ_KEY_PRESS,
                               2, "b", true, true) == 1);
    expect_chrome(router, "open-command-palette");
    assert(zz_input_router_key(router, ZZ_KEY_CHARACTER, '1', 0, 0, ZZ_KEY_PRESS,
                               8, "1", false, false) == 1);
    expect_chrome(router, "select-window-1");
    zz_input_router_free(router);
}

static void releases_pair_by_unshifted_key(void) {
    zz_input_router *router = new_router();
    activate(router, pane_owner(17, ZZ_SURFACE_TERMINAL));
    assert(zz_input_router_key(router, ZZ_KEY_CHARACTER, 'A', 'a', 0, ZZ_KEY_PRESS,
                               1, "A", true, true) == 1);
    expect_forward(router, 17, ZZ_KEY_CHARACTER, 'A', 0, ZZ_KEY_PRESS, 1, "A");
    assert(zz_input_router_key(router, ZZ_KEY_CHARACTER, 'a', 'a', 0, ZZ_KEY_RELEASE,
                               0, NULL, false, false) == 1);
    expect_forward(router, 17, ZZ_KEY_CHARACTER, 'a', 0, ZZ_KEY_RELEASE, 0, NULL);
    zz_input_router_free(router);
}

static void unsupported_action_becomes_native(void) {
    zz_chrome_keymap *keymap = zz_chrome_keymap_new();
    assert(keymap != NULL);
    assert(zz_chrome_keymap_bind(keymap, "ui", "C-b", "open-command-palette"));
    assert(zz_chrome_keymap_bind(keymap, "ui", "C-y", "open-command-palette"));
    zz_input_router *router = zz_input_router_new(keymap);
    assert(router != NULL);
    activate(router, pane_owner(17, ZZ_SURFACE_TERMINAL));
    assert(zz_input_router_unbind_action(router, "open-command-palette") >= 2);
    assert(zz_input_router_unbind_action(router, "open-command-palette") == 0);
    assert(zz_input_router_key(router, ZZ_KEY_CHARACTER, 'b', 0, 0, ZZ_KEY_PRESS,
                               2, "b", false, false) == 0);
    expect_empty(router);
    assert(zz_input_router_key(router, ZZ_KEY_CHARACTER, 'y', 0, 0, ZZ_KEY_PRESS,
                               2, "y", false, false) == 0);
    expect_empty(router);
    zz_input_router_free(router);
}

int main(void) {
    pane_keys_stay_native();
    sidebar_then_pending_pane();
    prefix_pairing_and_payloads();
    ui_precedes_prefix_and_owner();
    releases_pair_by_unshifted_key();
    unsupported_action_becomes_native();
    puts("input router scenarios 1, 2, 5, 7, 8 passed");
    return 0;
}
