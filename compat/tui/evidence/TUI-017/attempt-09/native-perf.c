#define _POSIX_C_SOURCE 200809L
#include <ghostty/vt.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

static double elapsed(struct timespec before, struct timespec after) {
    return (double)(after.tv_sec - before.tv_sec) +
           (double)(after.tv_nsec - before.tv_nsec) / 1000000000.0;
}

int main(void) {
    static const unsigned char row[] =
        "ABC\t\033[31mnamed\033[0m\t\033[38;5;1mindexed\033[0m\t"
        "\033[48;5;4mBG\033[0m\t\347\225\214\tTAIL\r\n";
    size_t length = (8u * 1024u * 1024u / (sizeof(row) - 1)) * (sizeof(row) - 1);
    unsigned char *payload = malloc(length);
    if (!payload) return 2;
    for (size_t offset = 0; offset < length; offset += sizeof(row) - 1)
        memcpy(payload + offset, row, sizeof(row) - 1);
    GhosttyOptimizeMode optimize;
    bool simd, kitty, tmux;
    if (ghostty_build_info(GHOSTTY_BUILD_INFO_OPTIMIZE, &optimize) != GHOSTTY_SUCCESS ||
        ghostty_build_info(GHOSTTY_BUILD_INFO_SIMD, &simd) != GHOSTTY_SUCCESS ||
        ghostty_build_info(GHOSTTY_BUILD_INFO_KITTY_GRAPHICS, &kitty) != GHOSTTY_SUCCESS ||
        ghostty_build_info(GHOSTTY_BUILD_INFO_TMUX_CONTROL_MODE, &tmux) != GHOSTTY_SUCCESS)
        return 3;
    GhosttyTerminal terminal;
    GhosttyTerminalOptions options = {.cols = 80, .rows = 23, .max_scrollback = 2000};
    if (ghostty_terminal_new(NULL, &terminal, options) != GHOSTTY_SUCCESS) return 4;
    struct timespec wall_start, wall_end, cpu_start, cpu_end;
    if (clock_gettime(CLOCK_MONOTONIC, &wall_start) ||
        clock_gettime(CLOCK_PROCESS_CPUTIME_ID, &cpu_start)) return 5;
    ghostty_terminal_vt_write(terminal, payload, length);
    if (clock_gettime(CLOCK_PROCESS_CPUTIME_ID, &cpu_end) ||
        clock_gettime(CLOCK_MONOTONIC, &wall_end)) return 6;
    ghostty_terminal_free(terminal);
    free(payload);
    printf("{\"bytes\":%zu,\"columns\":80,\"rows\":23,\"scrollback\":2000,"
           "\"optimize\":%d,\"simd\":%s,\"kitty\":%s,\"tmux\":%s,"
           "\"wall_seconds\":%.9f,\"cpu_seconds\":%.9f}\n",
           length, (int)optimize, simd ? "true" : "false", kitty ? "true" : "false",
           tmux ? "true" : "false", elapsed(wall_start, wall_end), elapsed(cpu_start, cpu_end));
    return 0;
}
