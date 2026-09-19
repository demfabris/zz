#define _DEFAULT_SOURCE
#define _POSIX_C_SOURCE 200809L
#include <ghostty/vt.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <linux/perf_event.h>
#include <sys/ioctl.h>
#include <sys/syscall.h>
#include <unistd.h>

static double elapsed(struct timespec before, struct timespec after) {
    return (double)(after.tv_sec - before.tv_sec) +
           (double)(after.tv_nsec - before.tv_nsec) / 1000000000.0;
}

static int instruction_counter(void) {
    struct perf_event_attr attr;
    memset(&attr, 0, sizeof attr);
    attr.size = sizeof attr;
    attr.type = PERF_TYPE_HARDWARE;
    attr.config = PERF_COUNT_HW_INSTRUCTIONS;
    attr.disabled = 1;
    attr.exclude_kernel = 1;
    attr.exclude_hv = 1;
    return (int)syscall(SYS_perf_event_open, &attr, 0, -1, -1, 0);
}

static size_t append(char *buffer, size_t offset, const char *text) {
    size_t length = strlen(text);
    memcpy(buffer + offset, text, length);
    return offset + length;
}

static void swap_tabs(char *row, bool spaces) {
    if (!spaces) return;
    for (char *c = row; *c; c++)
        if (*c == '\t') *c = ' ';
}

int main(int argc, char **argv) {
    if (argc != 3) return 2;
    const char *workload = argv[1];
    bool spaces = strcmp(argv[2], "spaces") == 0;
    char prefix[4096] = "";
    char row[512];
    size_t scrollback = 2000;
    bool reflow = false;
    if (strcmp(workload, "mixed") == 0) {
        strcpy(row, "ABC\t\033[31mnamed\033[0m\t\033[38;5;1mindexed\033[0m\t\033[48;5;4mBG\033[0m\t\347\225\214\tTAIL\r\n");
    } else if (strcmp(workload, "alltabs") == 0) {
        strcpy(row, "\t\t\t\t\t\t\t\t\t\tX\r\n");
    } else if (strcmp(workload, "htsevery") == 0) {
        size_t offset = append(prefix, 0, "\033[3g");
        for (int column = 1; column <= 80; column++) {
            char stop[32];
            snprintf(stop, sizeof stop, "\033[%dG\033H", column);
            offset = append(prefix, offset, stop);
        }
        append(prefix, offset, "\r");
        memset(row, '\t', 79);
        strcpy(row + 79, "X\r\n");
    } else if (strcmp(workload, "widestops") == 0) {
        strcpy(prefix, "\033[3g\033[1G\033H\033[61G\033H\r");
        strcpy(row, "\t\tX\r\n");
    } else if (strcmp(workload, "overwrite") == 0) {
        strcpy(row, "ABC\tDEF\tGHI\rabcdefghijklmnopqrs\r\n");
    } else if (strcmp(workload, "edits") == 0) {
        strcpy(row, "A\tB\tC\r\033[3G\033[2P\033[5G\033[3@\033[2X\r\n");
    } else if (strcmp(workload, "reflow") == 0) {
        strcpy(row, "ABC\t\033[31mnamed\033[0m\t\033[38;5;1mindexed\033[0m\t\033[48;5;4mBG\033[0m\t\347\225\214\tTAIL\tabcdefghijklmnopqrstuvwxyz\tEND\r\n");
        scrollback = 10000;
        reflow = true;
    } else {
        return 2;
    }
    swap_tabs(row, spaces);
    size_t row_length = strlen(row);
    size_t prefix_length = strlen(prefix);
    size_t length = prefix_length + (8u * 1024u * 1024u / row_length) * row_length;
    char *payload = malloc(length);
    if (!payload) return 2;
    memcpy(payload, prefix, prefix_length);
    for (size_t offset = prefix_length; offset < length; offset += row_length)
        memcpy(payload + offset, row, row_length);
    GhosttyTerminal terminal;
    GhosttyTerminalOptions options = {.cols = 80, .rows = 23, .max_scrollback = scrollback};
    if (ghostty_terminal_new(NULL, &terminal, options) != GHOSTTY_SUCCESS) return 4;
    struct timespec wall_start, wall_end, cpu_start, cpu_end;
    int counter = instruction_counter();
    long long instructions = -1;
    if (reflow) ghostty_terminal_vt_write(terminal, (const uint8_t *)payload, length);
    if (clock_gettime(CLOCK_MONOTONIC, &wall_start) ||
        clock_gettime(CLOCK_PROCESS_CPUTIME_ID, &cpu_start)) return 5;
    if (counter >= 0) {
        ioctl(counter, PERF_EVENT_IOC_RESET, 0);
        ioctl(counter, PERF_EVENT_IOC_ENABLE, 0);
    }
    if (reflow) {
        for (int i = 0; i < 10; i++) {
            if (ghostty_terminal_resize(terminal, 53, 23, 8, 16) != GHOSTTY_SUCCESS) return 7;
            if (ghostty_terminal_resize(terminal, 80, 23, 8, 16) != GHOSTTY_SUCCESS) return 7;
        }
    } else {
        ghostty_terminal_vt_write(terminal, (const uint8_t *)payload, length);
    }
    if (counter >= 0) {
        ioctl(counter, PERF_EVENT_IOC_DISABLE, 0);
        if (read(counter, &instructions, sizeof instructions) != sizeof instructions) instructions = -1;
        close(counter);
    }
    if (clock_gettime(CLOCK_PROCESS_CPUTIME_ID, &cpu_end) ||
        clock_gettime(CLOCK_MONOTONIC, &wall_end)) return 6;
    ghostty_terminal_free(terminal);
    free(payload);
    printf("{\"workload\":\"%s\",\"variant\":\"%s\",\"bytes\":%zu,\"wall_seconds\":%.9f,\"cpu_seconds\":%.9f,\"instructions\":%lld}\n",
           workload, argv[2], length, elapsed(wall_start, wall_end), elapsed(cpu_start, cpu_end), instructions);
    return 0;
}
