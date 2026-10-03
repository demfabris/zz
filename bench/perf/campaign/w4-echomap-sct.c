#define _GNU_SOURCE
#include <dlfcn.h>
#include <fcntl.h>
#include <poll.h>
#include <pthread.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/epoll.h>
#include <sys/select.h>
#include <sys/socket.h>
#include <sys/syscall.h>
#include <sys/uio.h>
#include <time.h>
#include <unistd.h>

enum { C_READ = 1, C_WRITE, C_READV, C_WRITEV, C_RECVMSG, C_SENDMSG, C_EPOLL, C_POLL, C_SELECT, C_PSELECT, C_EPOLLCTL, C_RECV, C_SEND, C_IOCTL };

struct rec { uint64_t t0, t1; int32_t tid, call, fd, ret; };
#define CAP (1 << 18)
static struct rec *buf;
static volatile uint32_t len;
static pthread_mutex_t mu = PTHREAD_MUTEX_INITIALIZER;
static int need_start = 1;
static int pid;

static uint64_t now(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (uint64_t)ts.tv_sec * 1000000000ull + ts.tv_nsec;
}

static void flush(void) {
    const char *dir = getenv("SCT_DIR");
    if (!dir || !buf) return;
    pthread_mutex_lock(&mu);
    uint32_t n = len;
    len = 0;
    if (!n) { pthread_mutex_unlock(&mu); return; }
    struct rec *copy = malloc(n * sizeof *copy);
    memcpy(copy, buf, n * sizeof *copy);
    pthread_mutex_unlock(&mu);
    char path[512];
    snprintf(path, sizeof path, "%s/sct-%d.txt", dir, (int)syscall(SYS_getpid));
    int fd = (int)syscall(SYS_open, path, O_WRONLY | O_CREAT | O_APPEND, 0644);
    if (fd >= 0) {
        char line[160];
        for (uint32_t i = 0; i < n; i++) {
            int k = snprintf(line, sizeof line, "%d %d %d %d %llu %llu\n", copy[i].tid, copy[i].call, copy[i].fd, copy[i].ret, (unsigned long long)copy[i].t0, (unsigned long long)copy[i].t1);
            syscall(SYS_write, fd, line, k);
        }
        syscall(SYS_close, fd);
    }
    free(copy);
}

static void *flusher(void *arg) {
    (void)arg;
    struct timespec ts = {0, 200000000};
    for (;;) {
        syscall(SYS_nanosleep, &ts, NULL);
        flush();
    }
    return NULL;
}

static void child(void) {
    need_start = 1;
    len = 0;
    pthread_mutex_init(&mu, NULL);
}

static void start(void) {
    if (!need_start) return;
    need_start = 0;
    if (!getenv("SCT_DIR")) return;
    if (!buf) buf = malloc(CAP * sizeof *buf);
    pid = (int)syscall(SYS_getpid);
    pthread_t th;
    pthread_attr_t at;
    pthread_attr_init(&at);
    pthread_attr_setdetachstate(&at, PTHREAD_CREATE_DETACHED);
    pthread_create(&th, &at, flusher, NULL);
}

__attribute__((constructor)) static void init(void) {
    pthread_atfork(NULL, NULL, child);
}

__attribute__((destructor)) static void fini(void) { flush(); }

static void rec(int call, int fd, long ret, uint64_t t0) {
    uint64_t t1 = now();
    if (!buf) return;
    pthread_mutex_lock(&mu);
    if (len < CAP) {
        buf[len++] = (struct rec){t0, t1, (int)syscall(SYS_gettid), call, fd, (int)ret};
    }
    pthread_mutex_unlock(&mu);
}

#define REAL(name) static __typeof__(name) *real_##name; if (!real_##name) real_##name = dlsym(RTLD_NEXT, #name)

ssize_t read(int fd, void *b, size_t n) { REAL(read); start(); uint64_t t = now(); ssize_t r = real_read(fd, b, n); rec(C_READ, fd, r, t); return r; }
ssize_t write(int fd, const void *b, size_t n) { REAL(write); start(); uint64_t t = now(); ssize_t r = real_write(fd, b, n); rec(C_WRITE, fd, r, t); return r; }
ssize_t readv(int fd, const struct iovec *v, int c) { REAL(readv); start(); uint64_t t = now(); ssize_t r = real_readv(fd, v, c); rec(C_READV, fd, r, t); return r; }
ssize_t writev(int fd, const struct iovec *v, int c) { REAL(writev); start(); uint64_t t = now(); ssize_t r = real_writev(fd, v, c); rec(C_WRITEV, fd, r, t); return r; }
ssize_t recvmsg(int fd, struct msghdr *m, int f) { REAL(recvmsg); start(); uint64_t t = now(); ssize_t r = real_recvmsg(fd, m, f); rec(C_RECVMSG, fd, r, t); return r; }
ssize_t sendmsg(int fd, const struct msghdr *m, int f) { REAL(sendmsg); start(); uint64_t t = now(); ssize_t r = real_sendmsg(fd, m, f); rec(C_SENDMSG, fd, r, t); return r; }
ssize_t recv(int fd, void *b, size_t n, int f) { REAL(recv); start(); uint64_t t = now(); ssize_t r = real_recv(fd, b, n, f); rec(C_RECV, fd, r, t); return r; }
ssize_t send(int fd, const void *b, size_t n, int f) { REAL(send); start(); uint64_t t = now(); ssize_t r = real_send(fd, b, n, f); rec(C_SEND, fd, r, t); return r; }
int epoll_wait(int e, struct epoll_event *ev, int m, int to) { REAL(epoll_wait); start(); uint64_t t = now(); int r = real_epoll_wait(e, ev, m, to); rec(C_EPOLL, to, r, t); return r; }
int epoll_pwait(int e, struct epoll_event *ev, int m, int to, const sigset_t *s) { REAL(epoll_pwait); start(); uint64_t t = now(); int r = real_epoll_pwait(e, ev, m, to, s); rec(C_EPOLL, to, r, t); return r; }
int epoll_ctl(int e, int op, int fd, struct epoll_event *ev) { REAL(epoll_ctl); start(); uint64_t t = now(); int r = real_epoll_ctl(e, op, fd, ev); rec(C_EPOLLCTL, fd, r, t); return r; }
int poll(struct pollfd *f, nfds_t n, int to) { REAL(poll); start(); uint64_t t = now(); int r = real_poll(f, n, to); rec(C_POLL, to, r, t); return r; }
int select(int n, fd_set *r_, fd_set *w, fd_set *e, struct timeval *to) { REAL(select); start(); uint64_t t = now(); int r = real_select(n, r_, w, e, to); rec(C_SELECT, n, r, t); return r; }
int pselect(int n, fd_set *r_, fd_set *w, fd_set *e, const struct timespec *to, const sigset_t *s) { REAL(pselect); start(); uint64_t t = now(); int r = real_pselect(n, r_, w, e, to, s); rec(C_PSELECT, n, r, t); return r; }
