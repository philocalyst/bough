// Present observer (the harness's MangoHud present hook, for macOS): timestamps every frame an app hands to
// the window server, into a shared mmap'ed log the runner reads (AWAGY_PRESENT_LOG). Observed:
//   -[CAMetalLayer nextDrawable]  (wgpu, gpui, Skia/Metal: one drawable per presented frame)
//   CGLFlushDrawable               (OpenGL: one flush per presented frame)
//   -[CALayer setContents:] with an IOSurface (bough: one surface published per presented frame)
// Clock: CLOCK_UPTIME_RAW nanoseconds. Layout: u64 count, then u64 timestamps, then u8 source tags.
#import <Foundation/Foundation.h>
#import <QuartzCore/QuartzCore.h>
#import <IOSurface/IOSurface.h>
#import <OpenGL/OpenGL.h>
#import <objc/runtime.h>
#include <stdatomic.h>
#include <sys/mman.h>
#include <fcntl.h>
#include <time.h>

#define CAP (1u << 20)
static _Atomic uint64_t *count;
static uint64_t *stamps;
static uint8_t *tags;

static void record(uint8_t tag) {
    if (!count) return;
    uint64_t i = atomic_fetch_add(count, 1);
    if (i < CAP) { stamps[i] = clock_gettime_nsec_np(CLOCK_UPTIME_RAW); tags[i] = tag; }
}

static id (*orig_next)(id, SEL);
static id hook_next(id self, SEL s) { id d = orig_next(self, s); if (d) record(1); return d; }
static void (*orig_contents)(id, SEL, id);
static void hook_contents(id self, SEL s, id v) {
    if (v && CFGetTypeID((__bridge CFTypeRef)v) == IOSurfaceGetTypeID()) record(3);
    orig_contents(self, s, v);
}
static CGLError hook_flush(CGLContextObj ctx) { record(2); return CGLFlushDrawable(ctx); }
__attribute__((used, section("__DATA,__interpose"))) static struct { const void *a, *b; } interpose[] = {
    {(const void *)hook_flush, (const void *)CGLFlushDrawable},
};

__attribute__((constructor)) static void init(void) {
    const char *path = getenv("AWAGY_PRESENT_LOG");
    if (!path) return;
    size_t len = 8 + CAP * 9;
    int fd = open(path, O_RDWR | O_CREAT | O_TRUNC, 0600);
    if (fd < 0 || ftruncate(fd, len) != 0) return;
    void *p = mmap(NULL, len, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    close(fd);
    if (p == MAP_FAILED) return;
    count = (_Atomic uint64_t *)p; stamps = (uint64_t *)((char *)p + 8); tags = (uint8_t *)((char *)p + 8 + CAP * 8);
    Method m = class_getInstanceMethod([CAMetalLayer class], @selector(nextDrawable));
    orig_next = (void *)method_setImplementation(m, (IMP)hook_next);
    Method c = class_getInstanceMethod([CALayer class], @selector(setContents:));
    orig_contents = (void *)method_setImplementation(c, (IMP)hook_contents);
}
