#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <termios.h>
#include <time.h>
#include <unistd.h>

static uint64_t now(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (uint64_t)ts.tv_sec * 1000000000ull + ts.tv_nsec;
}

#define N 400
static int rfd, wfd, rfd2, wfd2;
static volatile uint64_t t_write[N], t_wake[N], t_read[N], t_back[N];

static void *responder(void *arg) {
    (void)arg;
    char buf[64];
    for (int i = 0; i < N; i++) {
        struct pollfd p = {.fd = rfd, .events = POLLIN};
        while (poll(&p, 1, -1) < 0 && errno == EINTR) {}
        t_wake[i] = now();
        ssize_t n = read(rfd, buf, sizeof buf);
        t_read[i] = now();
        if (n <= 0) { perror("read"); exit(1); }
        if (wfd2 >= 0) {
            write(wfd2, buf, 1);
        }
    }
    return NULL;
}

static int cmp(const void *a, const void *b) {
    uint64_t x = *(const uint64_t *)a, y = *(const uint64_t *)b;
    return x < y ? -1 : x > y;
}

static void report(const char *name, uint64_t *v) {
    qsort(v, N, sizeof v[0], cmp);
    printf("%-28s p50 %7.1f us  p90 %7.1f  p99 %7.1f\n", name, v[N / 2] / 1e3, v[N * 9 / 10] / 1e3, v[N * 99 / 100] / 1e3);
}

static void run(const char *name, int r, int w, int r2, int w2, int gap_us) {
    rfd = r; wfd = w; rfd2 = r2; wfd2 = w2;
    pthread_t th;
    pthread_create(&th, NULL, responder, NULL);
    usleep(10000);
    uint64_t wake[N], readv[N], rtt[N];
    char buf[64];
    unsigned seed = 7;
    for (int i = 0; i < N; i++) {
        usleep(gap_us ? gap_us / 2 + rand_r(&seed) % gap_us : 0);
        t_write[i] = now();
        if (write(wfd, "q", 1) != 1) { perror("write"); exit(1); }
        if (rfd2 >= 0) {
            struct pollfd p = {.fd = rfd2, .events = POLLIN};
            while (poll(&p, 1, -1) < 0 && errno == EINTR) {}
            t_back[i] = now();
            read(rfd2, buf, sizeof buf);
        }
    }
    pthread_join(th, NULL);
    for (int i = 0; i < N; i++) {
        wake[i] = t_wake[i] - t_write[i];
        readv[i] = t_read[i] - t_write[i];
        rtt[i] = rfd2 >= 0 ? t_back[i] - t_write[i] : 0;
    }
    char label[96];
    snprintf(label, sizeof label, "%s write->poll", name);
    report(label, wake);
    snprintf(label, sizeof label, "%s write->read", name);
    report(label, readv);
    if (rfd2 >= 0) {
        snprintf(label, sizeof label, "%s round trip", name);
        report(label, rtt);
    }
}

static void pty(int *master, int *slave) {
    *master = posix_openpt(O_RDWR | O_NOCTTY);
    grantpt(*master);
    unlockpt(*master);
    *slave = open(ptsname(*master), O_RDWR | O_NOCTTY);
    struct termios t;
    tcgetattr(*slave, &t);
    cfmakeraw(&t);
    tcsetattr(*slave, TCSANOW, &t);
}

int main(int argc, char **argv) {
    int gap = argc > 1 ? atoi(argv[1]) : 30000;
    setvbuf(stdout, NULL, _IONBF, 0);
    printf("gap %d us (uniform %d..%d), %d samples, threads in one process\n", gap, gap / 2, gap * 3 / 2, N);
    int m, s;
    pty(&m, &s);
    run("pty master->slave", s, m, -1, -1, gap);
    close(m); close(s);
    pty(&m, &s);
    run("pty slave->master", m, s, -1, -1, gap);
    close(m); close(s);
    pty(&m, &s);
    run("pty m->s->m (echo)", s, m, m, s, gap);
    close(m); close(s);
    int sv[2];
    socketpair(AF_UNIX, SOCK_STREAM, 0, sv);
    run("unix stream", sv[1], sv[0], -1, -1, gap);
    close(sv[0]); close(sv[1]);
    int p[2];
    pipe(p);
    run("pipe (thread wake)", p[0], p[1], -1, -1, gap);
    close(p[0]); close(p[1]);
    int a[2], b[2];
    pipe(a); pipe(b);
    run("pipe ping-pong", a[0], a[1], b[0], b[1], gap);
    return 0;
}
