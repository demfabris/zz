#include <dispatch/dispatch.h>
#include <execinfo.h>
#include <mach-o/dyld.h>
#include <fcntl.h>
#include <libproc.h>
#include <poll.h>
#include <pthread.h>
#include <signal.h>
#include <stdarg.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/event.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <sys/select.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/sysctl.h>
#include <sys/uio.h>
#include <sys/wait.h>
#include <termios.h>
#include <time.h>
#include <unistd.h>

#define DYLD_INTERPOSE(_replacement, _replacee)                                              \
    __attribute__((used)) static struct {                                                    \
        const void *replacement;                                                             \
        const void *replacee;                                                                \
    } _interpose_##_replacee __attribute__((section("__DATA,__interpose"))) = {              \
        (const void *)(unsigned long)&_replacement, (const void *)(unsigned long)&_replacee};

#define NAMES                                                                                \
    X(read) X(write) X(readv) X(writev) X(pread) X(pwrite) X(send) X(sendto) X(sendmsg)      \
    X(recv) X(recvfrom) X(recvmsg) X(kevent) X(kevent64) X(poll) X(select) X(fcntl) X(ioctl) \
    X(setsockopt) X(getsockopt) X(getpeereid) X(getsockname) X(open) X(openat) X(close)      \
    X(dup) X(dup2) X(pipe) X(socketpair) X(socket) X(accept) X(connect) X(shutdown) X(stat)  \
    X(lstat) X(fstat) X(fstatat) X(readlink) X(access) X(proc_pidinfo) X(proc_pidpath)       \
    X(proc_pidfdinfo) X(proc_listchildpids) X(proc_pid_rusage) X(sysctl) X(sysctlbyname)     \
    X(kill) X(waitpid) X(wait4) X(getpid) X(tcgetpgrp) X(tcgetattr) X(tcsetattr) X(isatty)   \
    X(nanosleep) X(usleep) X(pthread_cond_signal) X(pthread_cond_broadcast)                  \
    X(pthread_cond_wait) X(pthread_cond_timedwait) X(dispatch_semaphore_signal)              \
    X(dispatch_semaphore_wait) X(__ulock_wait) X(__ulock_wake) X(os_sync_wait_on_address)    \
    X(os_sync_wake_by_address_any) X(os_sync_wake_by_address_all) X(mmap) X(munmap)          \
    X(madvise) X(getrusage) X(pthread_kill) X(sigaction) X(posix_spawn) X(fork) X(execve) X(kevent_blocked)

enum {
#define X(n) F_##n,
    NAMES
#undef X
    NF
};

static const char *fnames[] = {
#define X(n) #n,
    NAMES
#undef X
};

#define SLOTS 96
#define NAMELEN 48

struct slot {
    char name[NAMELEN];
    _Atomic uint64_t counts[NF];
};

struct table {
    uint64_t magic;
    uint64_t nf;
    uint64_t slots;
    _Atomic uint64_t used;
    char fnames[NF][24];
    struct slot slot[SLOTS];
};

static struct table *table;
static FILE *trace_file;
static _Atomic int lock;
static __thread int my_slot = -1;

static void map_table(void);

static void after_fork(void) {
    table = NULL;
    my_slot = -1;
    map_table();
}

__attribute__((constructor)) static void setup(void) {
    pthread_atfork(NULL, NULL, after_fork);
    map_table();
}

static void map_table(void) {
    const char *dir = getenv("ZZ_SYSCOUNT_DIR");
    if (!dir)
        return;
    char path[512];
    snprintf(path, sizeof path, "%s/%d.bin", dir, getpid());
    int fd = open(path, O_RDWR | O_CREAT | O_TRUNC, 0600);
    if (fd < 0)
        return;
    if (ftruncate(fd, sizeof(struct table)) != 0) {
        close(fd);
        return;
    }
    void *map = mmap(NULL, sizeof(struct table), PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    close(fd);
    if (map == MAP_FAILED)
        return;
    memset(map, 0, sizeof(struct table));
    if (getenv("ZZ_SYSCOUNT_LOG")) {
        snprintf(path, sizeof path, "%s/%d.log", dir, getpid());
        trace_file = fopen(path, "w");
        if (trace_file)
            setvbuf(trace_file, NULL, _IOLBF, 0);
    }
    table = map;
    table->nf = NF;
    table->slots = SLOTS;
    for (int i = 0; i < NF; i++)
        strncpy(table->fnames[i], fnames[i], 23);
    table->magic = 0x5a5a5343;
}

static int find_slot(void) {
    char name[NAMELEN] = {0};
    pthread_getname_np(pthread_self(), name, NAMELEN);
    if (!name[0])
        snprintf(name, NAMELEN, "%s", pthread_main_np() ? "<main>" : "<unnamed>");
    while (atomic_exchange(&lock, 1))
        ;
    uint64_t used = atomic_load(&table->used);
    int found = -1;
    for (uint64_t i = 0; i < used; i++)
        if (strncmp(table->slot[i].name, name, NAMELEN) == 0)
            found = (int)i;
    if (found < 0 && used < SLOTS) {
        strncpy(table->slot[used].name, name, NAMELEN - 1);
        found = (int)used;
        atomic_store(&table->used, used + 1);
    }
    atomic_store(&lock, 0);
    return found;
}


static void note(const char *fn, int fd, long len, const void *data, long shown) {
    if (!trace_file)
        return;
    struct stat st;
    const char *kind = "?";
    if (fstat(fd, &st) == 0)
        kind = S_ISSOCK(st.st_mode) ? "sock" : S_ISCHR(st.st_mode) ? "chr" : S_ISFIFO(st.st_mode) ? "fifo" : S_ISREG(st.st_mode) ? "reg" : "other";
    char name[NAMELEN] = {0};
    pthread_getname_np(pthread_self(), name, NAMELEN);
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    fprintf(trace_file, "%ld.%06ld %s %s fd=%d %s len=%ld", (long)ts.tv_sec, ts.tv_nsec / 1000, name, fn, fd, kind, len);
    if (data && len > 0) {
        fprintf(trace_file, " ");
        const unsigned char *b = data;
        for (long i = 0; i < len && i < shown; i++)
            fprintf(trace_file, (b[i] >= 32 && b[i] < 127) ? "%c" : "\\x%02x", b[i]);
    }
    fprintf(trace_file, "\n");
}

static void stack(const char *what) {
    void *frames[24];
    int n = backtrace(frames, 24);
    unsigned long slide = 0;
    for (uint32_t i = 0; i < _dyld_image_count(); i++) {
        const char *image = _dyld_get_image_name(i);
        if (image && strstr(image, "zz_cli")) {
            slide = (unsigned long)_dyld_get_image_vmaddr_slide(i);
            break;
        }
    }
    fprintf(trace_file, "BT %s slide=%lx", what, slide);
    for (int i = 1; i < n; i++)
        fprintf(trace_file, " %lx", (unsigned long)frames[i]);
    fprintf(trace_file, "\n");
}

static inline void bump(int f) {
    if (!table)
        return;
    if (my_slot < 0)
        my_slot = find_slot();
    if (my_slot >= 0)
        atomic_fetch_add_explicit(&table->slot[my_slot].counts[f], 1, memory_order_relaxed);
}

int my_pthread_setname_np(const char *name) {
    int r = pthread_setname_np(name);
    my_slot = -1;
    return r;
}
DYLD_INTERPOSE(my_pthread_setname_np, pthread_setname_np)

#define WRAP(ret, n, params, args)                                                           \
    ret my_##n params {                                                                      \
        bump(F_##n);                                                                         \
        return n args;                                                                       \
    }                                                                                        \
    DYLD_INTERPOSE(my_##n, n)

WRAP(ssize_t, readv, (int a, const struct iovec *b, int c), (a, b, c))
WRAP(ssize_t, pread, (int a, void *b, size_t c, off_t d), (a, b, c, d))
WRAP(ssize_t, pwrite, (int a, const void *b, size_t c, off_t d), (a, b, c, d))
WRAP(ssize_t, send, (int a, const void *b, size_t c, int d), (a, b, c, d))
WRAP(ssize_t, sendto, (int a, const void *b, size_t c, int d, const struct sockaddr *e, socklen_t f), (a, b, c, d, e, f))
WRAP(ssize_t, sendmsg, (int a, const struct msghdr *b, int c), (a, b, c))
WRAP(ssize_t, recv, (int a, void *b, size_t c, int d), (a, b, c, d))
WRAP(ssize_t, recvfrom, (int a, void *b, size_t c, int d, struct sockaddr *e, socklen_t *f), (a, b, c, d, e, f))
WRAP(ssize_t, recvmsg, (int a, struct msghdr *b, int c), (a, b, c))
WRAP(int, kevent64, (int a, const struct kevent64_s *b, int c, struct kevent64_s *d, int e, unsigned int f, const struct timespec *g), (a, b, c, d, e, f, g))
WRAP(int, poll, (struct pollfd *a, nfds_t b, int c), (a, b, c))
WRAP(int, select, (int a, fd_set *b, fd_set *c, fd_set *d, struct timeval *e), (a, b, c, d, e))
WRAP(int, setsockopt, (int a, int b, int c, const void *d, socklen_t e), (a, b, c, d, e))
WRAP(int, getsockopt, (int a, int b, int c, void *d, socklen_t *e), (a, b, c, d, e))
WRAP(int, getpeereid, (int a, uid_t *b, gid_t *c), (a, b, c))
WRAP(int, getsockname, (int a, struct sockaddr *b, socklen_t *c), (a, b, c))
WRAP(int, openat, (int a, const char *b, int c, int d), (a, b, c, d))
WRAP(int, close, (int a), (a))
WRAP(int, dup, (int a), (a))
WRAP(int, dup2, (int a, int b), (a, b))
WRAP(int, pipe, (int a[2]), (a))
WRAP(int, socketpair, (int a, int b, int c, int d[2]), (a, b, c, d))
WRAP(int, socket, (int a, int b, int c), (a, b, c))
WRAP(int, accept, (int a, struct sockaddr *b, socklen_t *c), (a, b, c))
WRAP(int, connect, (int a, const struct sockaddr *b, socklen_t c), (a, b, c))
WRAP(int, shutdown, (int a, int b), (a, b))
WRAP(int, stat, (const char *a, struct stat *b), (a, b))
WRAP(int, lstat, (const char *a, struct stat *b), (a, b))
WRAP(int, fstat, (int a, struct stat *b), (a, b))
WRAP(int, fstatat, (int a, const char *b, struct stat *c, int d), (a, b, c, d))
WRAP(ssize_t, readlink, (const char *restrict a, char *restrict b, size_t c), (a, b, c))
WRAP(int, access, (const char *a, int b), (a, b))
WRAP(int, proc_pidinfo, (int a, int b, uint64_t c, void *d, int e), (a, b, c, d, e))
WRAP(int, proc_pidpath, (int a, void *b, uint32_t c), (a, b, c))
WRAP(int, proc_pidfdinfo, (int a, int b, int c, void *d, int e), (a, b, c, d, e))
WRAP(int, proc_listchildpids, (pid_t a, void *b, int c), (a, b, c))
WRAP(int, proc_pid_rusage, (int a, int b, rusage_info_t *c), (a, b, c))
WRAP(int, sysctl, (int *a, u_int b, void *c, size_t *d, void *e, size_t f), (a, b, c, d, e, f))
WRAP(int, sysctlbyname, (const char *a, void *b, size_t *c, void *d, size_t e), (a, b, c, d, e))
WRAP(int, kill, (pid_t a, int b), (a, b))
WRAP(pid_t, waitpid, (pid_t a, int *b, int c), (a, b, c))
WRAP(pid_t, wait4, (pid_t a, int *b, int c, struct rusage *d), (a, b, c, d))
WRAP(pid_t, getpid, (void), ())
WRAP(pid_t, tcgetpgrp, (int a), (a))
WRAP(int, tcgetattr, (int a, struct termios *b), (a, b))
WRAP(int, tcsetattr, (int a, int b, const struct termios *c), (a, b, c))
WRAP(int, isatty, (int a), (a))
WRAP(int, nanosleep, (const struct timespec *a, struct timespec *b), (a, b))
WRAP(int, usleep, (useconds_t a), (a))
WRAP(int, pthread_cond_signal, (pthread_cond_t *a), (a))
WRAP(int, pthread_cond_broadcast, (pthread_cond_t *a), (a))
WRAP(int, pthread_cond_wait, (pthread_cond_t *a, pthread_mutex_t *b), (a, b))
WRAP(int, pthread_cond_timedwait, (pthread_cond_t *a, pthread_mutex_t *b, const struct timespec *c), (a, b, c))
WRAP(intptr_t, dispatch_semaphore_signal, (dispatch_semaphore_t a), (a))
WRAP(intptr_t, dispatch_semaphore_wait, (dispatch_semaphore_t a, dispatch_time_t b), (a, b))
WRAP(void *, mmap, (void *a, size_t b, int c, int d, int e, off_t f), (a, b, c, d, e, f))
WRAP(int, munmap, (void *a, size_t b), (a, b))
WRAP(int, madvise, (void *a, size_t b, int c), (a, b, c))
WRAP(int, getrusage, (int a, struct rusage *b), (a, b))
WRAP(int, pthread_kill, (pthread_t a, int b), (a, b))
WRAP(int, sigaction, (int a, const struct sigaction *restrict b, struct sigaction *restrict c), (a, b, c))
WRAP(pid_t, fork, (void), ())
WRAP(int, execve, (const char *a, char *const b[], char *const c[]), (a, b, c))

extern int __ulock_wait(uint32_t, void *, uint64_t, uint32_t);
extern int __ulock_wake(uint32_t, void *, uint64_t);
WRAP(int, __ulock_wait, (uint32_t a, void *b, uint64_t c, uint32_t d), (a, b, c, d))
WRAP(int, __ulock_wake, (uint32_t a, void *b, uint64_t c), (a, b, c))

extern int os_sync_wait_on_address(void *, uint64_t, size_t, uint32_t);
extern int os_sync_wake_by_address_any(void *, size_t, uint32_t);
extern int os_sync_wake_by_address_all(void *, size_t, uint32_t);
WRAP(int, os_sync_wait_on_address, (void *a, uint64_t b, size_t c, uint32_t d), (a, b, c, d))
WRAP(int, os_sync_wake_by_address_any, (void *a, size_t b, uint32_t c), (a, b, c))
WRAP(int, os_sync_wake_by_address_all, (void *a, size_t b, uint32_t c), (a, b, c))

extern int posix_spawn(pid_t *restrict, const char *restrict, const void *, const void *, char *const[], char *const[]);
WRAP(int, posix_spawn, (pid_t *restrict a, const char *restrict b, const void *c, const void *d, char *const e[], char *const f[]), (a, b, c, d, e, f))

ssize_t my_read(int a, void *b, size_t c) {
    bump(F_read);
    ssize_t r = read(a, b, c);
    note("read", a, r, b, 24);
    return r;
}
DYLD_INTERPOSE(my_read, read)

ssize_t my_write(int a, const void *b, size_t c) {
    bump(F_write);
    ssize_t r = write(a, b, c);
    note("write", a, r, b, 40);
    if (trace_file && c == 1 && getenv("ZZ_SYSCOUNT_BT"))
        stack("pipe");
    return r;
}
DYLD_INTERPOSE(my_write, write)

ssize_t my_writev(int a, const struct iovec *b, int c) {
    bump(F_writev);
    ssize_t r = writev(a, b, c);
    note("writev", a, r, c > 0 ? b[0].iov_base : NULL, 40);
    return r;
}
DYLD_INTERPOSE(my_writev, writev)

int my_kevent(int kq, const struct kevent *changes, int nchanges, struct kevent *events, int nevents, const struct timespec *timeout) {
    bump(F_kevent);
    struct timespec t0, t1;
    clock_gettime(CLOCK_MONOTONIC, &t0);
    int r = kevent(kq, changes, nchanges, events, nevents, timeout);
    clock_gettime(CLOCK_MONOTONIC, &t1);
    if ((t1.tv_sec - t0.tv_sec) * 1000000000L + (t1.tv_nsec - t0.tv_nsec) > 20000)
        bump(F_kevent_blocked);
    if (trace_file) {
        char name[NAMELEN] = {0};
        pthread_getname_np(pthread_self(), name, NAMELEN);
        struct timespec ts;
        clock_gettime(CLOCK_MONOTONIC, &ts);
        char buf[1024];
        int at = snprintf(buf, sizeof buf, "%ld.%06ld %s kevent kq=%d r=%d to=%ld changes:", (long)ts.tv_sec, ts.tv_nsec / 1000, name, kq, r, timeout ? (long)(timeout->tv_sec * 1000 + timeout->tv_nsec / 1000000) : -1L);
        for (int i = 0; i < nchanges && at < 900; i++)
            at += snprintf(buf + at, sizeof buf - at, " [%lu f=%d fl=%x ff=%x]", (unsigned long)changes[i].ident, changes[i].filter, changes[i].flags, changes[i].fflags);
        at += snprintf(buf + at, sizeof buf - at, " events:");
        for (int i = 0; i < r && at < 1000; i++)
            at += snprintf(buf + at, sizeof buf - at, " [%lu f=%d]", (unsigned long)events[i].ident, events[i].filter);
        fprintf(trace_file, "%s\n", buf);
        if (nchanges == 1 && changes[0].filter == EVFILT_USER && getenv("ZZ_SYSCOUNT_BT"))
            stack("wake");
    }
    return r;
}
DYLD_INTERPOSE(my_kevent, kevent)

int my_fcntl(int fd, int cmd, ...) {
    va_list ap;
    va_start(ap, cmd);
    void *arg = va_arg(ap, void *);
    va_end(ap);
    bump(F_fcntl);
    return fcntl(fd, cmd, arg);
}
DYLD_INTERPOSE(my_fcntl, fcntl)

int my_ioctl(int fd, unsigned long req, ...) {
    va_list ap;
    va_start(ap, req);
    void *arg = va_arg(ap, void *);
    va_end(ap);
    bump(F_ioctl);
    int r = ioctl(fd, req, arg);
    note("ioctl", fd, (long)req, NULL, 0);
    return r;
}
DYLD_INTERPOSE(my_ioctl, ioctl)

int my_open(const char *path, int flags, ...) {
    va_list ap;
    va_start(ap, flags);
    int mode = va_arg(ap, int);
    va_end(ap);
    bump(F_open);
    return open(path, flags, mode);
}
DYLD_INTERPOSE(my_open, open)
