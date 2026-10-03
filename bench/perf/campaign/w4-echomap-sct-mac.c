#include <fcntl.h>
#include <poll.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/event.h>
#include <sys/select.h>
#include <sys/socket.h>
#include <sys/uio.h>
#include <time.h>
#include <unistd.h>

#define INTERPOSE(rep, orig) __attribute__((used)) static struct { const void *r; const void *o; } interpose_##orig __attribute__((section("__DATA,__interpose"))) = {(const void *)(unsigned long)&rep, (const void *)(unsigned long)&orig}

enum { C_READ = 1, C_WRITE, C_READV, C_WRITEV, C_RECVMSG, C_SENDMSG, C_EPOLL, C_POLL, C_SELECT };

struct rec { uint64_t t0, t1; int32_t tid, call, fd, ret; };
#define CAP (1 << 18)
static struct rec *buf;
static uint32_t len;
static pthread_mutex_t mu = PTHREAD_MUTEX_INITIALIZER;
static int need_start = 1;
static int flushing;

static uint64_t now(void) { return clock_gettime_nsec_np(CLOCK_UPTIME_RAW); }

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
    snprintf(path, sizeof path, "%s/sct-%d.txt", dir, (int)getpid());
    flushing = 1;
    FILE *f = fopen(path, "a");
    if (f) {
        for (uint32_t i = 0; i < n; i++) {
            fprintf(f, "%d %d %d %d %llu %llu\n", copy[i].tid, copy[i].call, copy[i].fd, copy[i].ret, (unsigned long long)copy[i].t0, (unsigned long long)copy[i].t1);
        }
        fclose(f);
    }
    flushing = 0;
    free(copy);
}

static void *flusher(void *arg) {
    (void)arg;
    for (;;) {
        usleep(200000);
        flush();
    }
    return NULL;
}

static void child(void) { need_start = 1; len = 0; pthread_mutex_init(&mu, NULL); }

static void start(void) {
    if (!need_start) return;
    need_start = 0;
    if (!getenv("SCT_DIR")) return;
    if (!buf) buf = malloc(CAP * sizeof *buf);
    pthread_t th;
    pthread_attr_t at;
    pthread_attr_init(&at);
    pthread_attr_setdetachstate(&at, PTHREAD_CREATE_DETACHED);
    pthread_create(&th, &at, flusher, NULL);
}

__attribute__((constructor)) static void init(void) { pthread_atfork(NULL, NULL, child); }
__attribute__((destructor)) static void fini(void) { flush(); }

static void rec(int call, int fd, long ret, uint64_t t0) {
    uint64_t t1 = now();
    if (!buf || flushing) return;
    uint64_t tid = 0;
    pthread_threadid_np(NULL, &tid);
    pthread_mutex_lock(&mu);
    if (len < CAP) buf[len++] = (struct rec){t0, t1, (int32_t)tid, call, fd, (int32_t)ret};
    pthread_mutex_unlock(&mu);
}

static ssize_t my_read(int fd, void *b, size_t n) { start(); uint64_t t = now(); ssize_t r = read(fd, b, n); rec(C_READ, fd, r, t); return r; }
static ssize_t my_write(int fd, const void *b, size_t n) { start(); uint64_t t = now(); ssize_t r = write(fd, b, n); rec(C_WRITE, fd, r, t); return r; }
static ssize_t my_readv(int fd, const struct iovec *v, int c) { start(); uint64_t t = now(); ssize_t r = readv(fd, v, c); rec(C_READV, fd, r, t); return r; }
static ssize_t my_writev(int fd, const struct iovec *v, int c) { start(); uint64_t t = now(); ssize_t r = writev(fd, v, c); rec(C_WRITEV, fd, r, t); return r; }
static ssize_t my_recvmsg(int fd, struct msghdr *m, int f) { start(); uint64_t t = now(); ssize_t r = recvmsg(fd, m, f); rec(C_RECVMSG, fd, r, t); return r; }
static ssize_t my_sendmsg(int fd, const struct msghdr *m, int f) { start(); uint64_t t = now(); ssize_t r = sendmsg(fd, m, f); rec(C_SENDMSG, fd, r, t); return r; }
static int my_kevent(int kq, const struct kevent *c, int nc, struct kevent *e, int ne, const struct timespec *to) { start(); uint64_t t = now(); int r = kevent(kq, c, nc, e, ne, to); rec(C_EPOLL, to ? (int)(to->tv_sec * 1000 + to->tv_nsec / 1000000) : -1, r, t); return r; }
static int my_poll(struct pollfd *f, nfds_t n, int to) { start(); uint64_t t = now(); int r = poll(f, n, to); rec(C_POLL, to, r, t); return r; }
extern int select_ext(int, fd_set *, fd_set *, fd_set *, struct timeval *) __asm("_select$DARWIN_EXTSN");
static int my_select_ext(int n, fd_set *r_, fd_set *w, fd_set *e, struct timeval *to) { start(); uint64_t t = now(); int r = select_ext(n, r_, w, e, to); rec(C_SELECT, n, r, t); return r; }
static int my_select(int n, fd_set *r_, fd_set *w, fd_set *e, struct timeval *to) { start(); uint64_t t = now(); int r = select(n, r_, w, e, to); rec(C_SELECT, n, r, t); return r; }

INTERPOSE(my_read, read);
INTERPOSE(my_write, write);
INTERPOSE(my_readv, readv);
INTERPOSE(my_writev, writev);
INTERPOSE(my_recvmsg, recvmsg);
INTERPOSE(my_sendmsg, sendmsg);
INTERPOSE(my_kevent, kevent);
INTERPOSE(my_poll, poll);
INTERPOSE(my_select, select);
INTERPOSE(my_select_ext, select_ext);
