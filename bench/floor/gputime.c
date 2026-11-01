// Injected (DYLD_INSERT_LIBRARIES) into any Metal process: the process's GPU time as the I/O Registry
// accounts it per GPU client (AppUsage.accumulatedGPUTime of the accelerator's user clients, what Activity
// Monitor shows). No root, no entitlement. task_info(TASK_POWER_INFO_V2).gpu_energy is 0 on Apple silicon.
// A client's accounting disappears when the process releases its device (wgpu does before exit), so a
// thread polls the accelerator's children every GPUTIME_POLL_US (default 2000) and keeps each client's last
// value; at exit the sum is printed. The poller costs CPU: never measure instructions with this injected.
// The GPU's mean active clock over the process's life comes from IOReport ("GPU Stats" / GPUPH residency,
// state frequencies from pmgr voltage-states9, as zgx's macsample): Mcycles = GPU seconds x MHz. It is the
// whole GPU's clock (every process), so it normalises DVFS only roughly.
#include <CoreFoundation/CoreFoundation.h>
#include <IOKit/IOKitLib.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
typedef struct IOReportSubscription *IOReportSubscriptionRef;
extern CFMutableDictionaryRef IOReportCopyChannelsInGroup(CFStringRef, CFStringRef, uint64_t, uint64_t, uint64_t);
extern IOReportSubscriptionRef IOReportCreateSubscription(void *, CFMutableDictionaryRef, CFMutableDictionaryRef *, uint64_t, CFTypeRef);
extern CFDictionaryRef IOReportCreateSamples(IOReportSubscriptionRef, CFMutableDictionaryRef, CFTypeRef);
extern CFDictionaryRef IOReportCreateSamplesDelta(CFDictionaryRef, CFDictionaryRef, CFTypeRef);
extern CFStringRef IOReportChannelGetGroup(CFDictionaryRef);
extern CFStringRef IOReportChannelGetSubGroup(CFDictionaryRef);
extern CFStringRef IOReportChannelGetChannelName(CFDictionaryRef);
extern int32_t IOReportStateGetCount(CFDictionaryRef);
extern int64_t IOReportStateGetResidency(CFDictionaryRef, int32_t);
static IOReportSubscriptionRef sub;
static CFMutableDictionaryRef subbed;
static CFDictionaryRef s0;
static double mhz[32];
static int nmhz;
static void power_init(void) {
    CFMutableDictionaryRef ch = IOReportCopyChannelsInGroup(CFSTR("GPU Stats"), NULL, 0, 0, 0);
    if (!ch) return;
    sub = IOReportCreateSubscription(NULL, ch, &subbed, 0, NULL);
    if (!sub) return;
    io_service_t pmgr = IOServiceGetMatchingService(kIOMainPortDefault, IOServiceNameMatching("pmgr"));
    CFDataRef d = pmgr ? IORegistryEntryCreateCFProperty(pmgr, CFSTR("voltage-states9"), kCFAllocatorDefault, 0) : NULL;
    if (d) {
        const uint8_t *b = CFDataGetBytePtr(d);
        for (CFIndex i = 0; i + 8 <= CFDataGetLength(d) && nmhz < 32; i += 8) mhz[nmhz++] = (double)(*(const uint32_t *)(b + i)) / 1e6;
        CFRelease(d);
    }
    if (pmgr) IOObjectRelease(pmgr);
    s0 = IOReportCreateSamples(sub, subbed, NULL);
}
static double power_mhz(void) {
    if (!sub || !s0) return 0;
    CFDictionaryRef s1 = IOReportCreateSamples(sub, subbed, NULL);
    CFDictionaryRef dl = s1 ? IOReportCreateSamplesDelta(s0, s1, NULL) : NULL;
    double active = 0, weighted = 0;
    CFArrayRef list = dl ? CFDictionaryGetValue(dl, CFSTR("IOReportChannels")) : NULL;
    for (CFIndex i = 0; list && i < CFArrayGetCount(list); i++) {
        CFDictionaryRef c = CFArrayGetValueAtIndex(list, i);
        CFStringRef name = IOReportChannelGetChannelName(c), sg = IOReportChannelGetSubGroup(c);
        if (!name || !sg || CFStringCompare(name, CFSTR("GPUPH"), 0) != 0 || CFStringCompare(sg, CFSTR("GPU Performance States"), 0) != 0) continue;
        int n = IOReportStateGetCount(c);
        for (int k = 1; k < n && k < nmhz; k++) {
            double r = (double)IOReportStateGetResidency(c, k);
            active += r;
            weighted += r * mhz[k];
        }
    }
    return active > 0 ? weighted / active : 0;
}
#define MAXC 64
static uint64_t ids[MAXC];
static double vals[MAXC];
static int nc;
static pthread_mutex_t mu = PTHREAD_MUTEX_INITIALIZER;
static char want[32];
static io_service_t accel;
static void poll_once(void) {
    io_iterator_t it = 0;
    if (IORegistryEntryGetChildIterator(accel, kIOServicePlane, &it) != KERN_SUCCESS) return;
    io_object_t e;
    while ((e = IOIteratorNext(it))) {
        CFStringRef cr = IORegistryEntryCreateCFProperty(e, CFSTR("IOUserClientCreator"), kCFAllocatorDefault, 0);
        char buf[128] = {0};
        if (cr && CFGetTypeID(cr) == CFStringGetTypeID() && CFStringGetCString(cr, buf, sizeof buf, kCFStringEncodingUTF8) && strncmp(buf, want, strlen(want)) == 0) {
            CFArrayRef u = IORegistryEntryCreateCFProperty(e, CFSTR("AppUsage"), kCFAllocatorDefault, 0);
            double t = 0;
            if (u && CFGetTypeID(u) == CFArrayGetTypeID()) {
                for (CFIndex i = 0; i < CFArrayGetCount(u); i++) {
                    CFDictionaryRef d = CFArrayGetValueAtIndex(u, i);
                    CFNumberRef n = CFDictionaryGetValue(d, CFSTR("accumulatedGPUTime"));
                    long long v = 0;
                    if (n && CFNumberGetValue(n, kCFNumberLongLongType, &v)) t += (double)v / 1e9;
                }
            }
            if (u) CFRelease(u);
            uint64_t id = 0;
            IORegistryEntryGetRegistryEntryID(e, &id);
            pthread_mutex_lock(&mu);
            int k = 0;
            while (k < nc && ids[k] != id) k++;
            if (k == nc && nc < MAXC) { ids[nc] = id; vals[nc] = 0; nc++; }
            if (k < MAXC && t > vals[k]) vals[k] = t;
            pthread_mutex_unlock(&mu);
        }
        if (cr) CFRelease(cr);
        IOObjectRelease(e);
    }
    IOObjectRelease(it);
}
static void *poller(void *arg) {
    useconds_t us = (useconds_t)(uintptr_t)arg;
    for (;;) { poll_once(); usleep(us); }
    return 0;
}
static void report(void) {
    usleep(20000);
    poll_once();
    double s = 0;
    pthread_mutex_lock(&mu);
    for (int k = 0; k < nc; k++) s += vals[k];
    pthread_mutex_unlock(&mu);
    dprintf(2, "gputime: gpu_seconds=%.6f clients=%d gpu_mhz=%.1f\n", s, nc, power_mhz());
}
__attribute__((constructor)) static void init(void) {
    snprintf(want, sizeof want, "pid %d,", getpid());
    power_init();
    accel = IOServiceGetMatchingService(kIOMainPortDefault, IOServiceMatching("IOAccelerator"));
    const char *p = getenv("GPUTIME_POLL_US");
    uintptr_t us = p ? (uintptr_t)atoi(p) : 2000;
    pthread_t t;
    pthread_create(&t, 0, poller, (void *)us);
    pthread_detach(t);
    atexit(report);
}
