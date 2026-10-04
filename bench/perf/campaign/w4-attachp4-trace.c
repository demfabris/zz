#define _GNU_SOURCE
#include <errno.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ptrace.h>
#include <sys/types.h>
#include <sys/user.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#define NR 512
#define TIDS 256

struct slot { pid_t tid; int inside; long nr; uint64_t t0; long a0, a1, a2; char link[48]; uint64_t count[NR]; uint64_t ns[NR]; };
struct ev { uint64_t t; pid_t tid; long nr, a0, a1, a2, ret; char link[48]; };
#define EVS (1 << 16)
static struct ev evs[EVS];
static unsigned nevs;
static int timeline;
static struct slot slots[TIDS];
static volatile sig_atomic_t dump_now;
static const char *out_path;
static pid_t root;

static uint64_t now(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (uint64_t)ts.tv_sec * 1000000000ull + ts.tv_nsec;
}

static struct slot *slot_for(pid_t tid) {
    for (int i = 0; i < TIDS; i++) if (slots[i].tid == tid) return &slots[i];
    for (int i = 0; i < TIDS; i++) if (slots[i].tid == 0) { slots[i].tid = tid; return &slots[i]; }
    return NULL;
}

static void on_usr1(int sig) { (void)sig; dump_now = 1; }

static void dump(void) {
    FILE *f = fopen(out_path, "a");
    if (!f) return;
    fprintf(f, "dump %llu\n", (unsigned long long)now());
    for (int i = 0; i < TIDS; i++) {
        if (!slots[i].tid) continue;
        char path[64], comm[64] = "?";
        snprintf(path, sizeof path, "/proc/%d/task/%d/comm", root, slots[i].tid);
        FILE *c = fopen(path, "r");
        if (c) { if (fgets(comm, sizeof comm, c)) comm[strcspn(comm, "\n")] = 0; fclose(c); }
        for (char *p = comm; *p; p++) if (*p == ' ') *p = '_';
        for (int n = 0; n < NR; n++) {
            if (slots[i].count[n]) fprintf(f, "%d %s %d %llu %llu\n", slots[i].tid, comm, n, (unsigned long long)slots[i].count[n], (unsigned long long)slots[i].ns[n]);
            slots[i].count[n] = 0;
            slots[i].ns[n] = 0;
        }
    }
    for (unsigned i = 0; i < nevs; i++)
        fprintf(f, "ev %llu %d %ld %ld %ld %ld %ld %s\n", (unsigned long long)evs[i].t, evs[i].tid, evs[i].nr, evs[i].a0, evs[i].a1, evs[i].a2, evs[i].ret, evs[i].link[0] ? evs[i].link : "-");
    nevs = 0;
    fprintf(f, "end\n");
    fclose(f);
}

int main(int argc, char **argv) {
    if (argc < 3) { fprintf(stderr, "usage: %s out cmd...\n", argv[0]); return 2; }
    out_path = argv[1];
    timeline = getenv("TIMELINE") != NULL;
    pid_t child = fork();
    if (child == 0) {
        ptrace(PTRACE_TRACEME, 0, 0, 0);
        raise(SIGSTOP);
        execvp(argv[2], argv + 2);
        _exit(127);
    }
    root = child;
    struct sigaction sa = {0};
    sa.sa_handler = on_usr1;
    sigaction(SIGUSR1, &sa, NULL);
    int status;
    waitpid(child, &status, 0);
    ptrace(PTRACE_SETOPTIONS, child, 0, PTRACE_O_TRACESYSGOOD | PTRACE_O_TRACECLONE | PTRACE_O_TRACEEXEC | PTRACE_O_EXITKILL | (getenv("FORKS") ? PTRACE_O_TRACEFORK | PTRACE_O_TRACEVFORK : 0));
    ptrace(PTRACE_SYSCALL, child, 0, 0);
    int live = 1;
    while (live > 0) {
        if (dump_now) { dump_now = 0; dump(); }
        pid_t tid = waitpid(-1, &status, __WALL);
        if (tid < 0) {
            if (errno == EINTR) continue;
            break;
        }
        if (WIFEXITED(status) || WIFSIGNALED(status)) {
            struct slot *s = slot_for(tid);
            if (s) s->inside = 0;
            if (tid == child) break;
            continue;
        }
        if (!WIFSTOPPED(status)) continue;
        int sig = WSTOPSIG(status);
        int event = status >> 16;
        int inject = 0;
        if (sig == (SIGTRAP | 0x80)) {
            struct slot *s = slot_for(tid);
            if (s) {
                if (!s->inside) {
                    struct user_regs_struct regs;
                    ptrace(PTRACE_GETREGS, tid, 0, &regs);
                    s->nr = (long)regs.orig_rax;
                    s->inside = 1;
                    s->t0 = now();
                    s->a0 = (long)regs.rdi;
                    s->a1 = (long)regs.rsi;
                    s->a2 = (long)regs.rdx;
                    s->link[0] = 0;
                    if (timeline && (s->nr == 59 || s->nr == 322)) {
                        char path[64];
                        snprintf(path, sizeof path, "/proc/%d/mem", tid);
                        FILE *m = fopen(path, "r");
                        s->link[0] = 0;
                        if (m) {
                            if (fseek(m, s->nr == 59 ? s->a0 : s->a1, SEEK_SET) == 0) {
                                size_t n = fread(s->link, 1, sizeof s->link - 1, m);
                                s->link[n] = 0;
                            }
                            fclose(m);
                        }
                        for (char *p = s->link; *p; p++) if (*p == ' ') *p = '_';
                        if (!s->link[0]) strcpy(s->link, "exec?");
                    } else if (timeline && s->a0 >= 0 && s->a0 < 4096) {
                        char path[64];
                        snprintf(path, sizeof path, "/proc/%d/fd/%ld", root, s->a0);
                        ssize_t n = readlink(path, s->link, sizeof s->link - 1);
                        s->link[n > 0 ? n : 0] = 0;
                        for (char *p = s->link; *p; p++) if (*p == ' ') *p = '_';
                    }
                    if (s->nr >= 0 && s->nr < NR) s->count[s->nr]++;
                } else {
                    s->inside = 0;
                    if (s->nr >= 0 && s->nr < NR) s->ns[s->nr] += now() - s->t0;
                    if (timeline && nevs < EVS) {
                        struct user_regs_struct regs;
                        ptrace(PTRACE_GETREGS, tid, 0, &regs);
                        struct ev *e = &evs[nevs++];
                        e->t = s->t0;
                        e->tid = tid;
                        e->nr = s->nr;
                        e->a0 = s->a0;
                        e->a1 = s->a1;
                        e->a2 = s->a2;
                        e->ret = (long)regs.rax;
                        memcpy(e->link, s->link, sizeof e->link);
                    }
                }
            }
        } else if (sig == SIGTRAP && event) {
            inject = 0;
        } else if (sig == SIGSTOP) {
            inject = 0;
        } else {
            inject = sig;
        }
        ptrace(PTRACE_SYSCALL, tid, 0, (void *)(long)inject);
    }
    dump();
    return 0;
}
