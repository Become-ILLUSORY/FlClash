/*
 * libclash.h — FlClash 与内核之间的 C ABI（meow-rs 实现）。
 *
 * 本头文件替代原 cgo 自动生成的同名头文件，保持导出符号与
 * `android/core/src/main/cpp/core.cpp` 及 `android/core/build.gradle.kts`
 * 期望一致，从而 JNI 层无需改动。
 */
#pragma once

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ---- 由 JNI 层注入的回调（JNI_OnLoad 里赋值） ---- */

extern void (*release_object_func)(void *obj);
extern void (*free_string_func)(char *data);
extern void (*protect_func)(void *tun_interface, int fd);
extern char *(*resolve_process_func)(void *tun_interface, int protocol,
                                     const char *source, const char *target,
                                     int uid);
extern void (*result_func)(void *invoke_interface, const char *data);

/* ---- FlClash 调用的内核导出函数 ---- */

/* 启动 TUN。callback 为 TunInterface（protect/resolverProcess 回调宿主）。
 * fd 是 Android VpnService 创建的外部 socket fd */
extern void startTUN(void *callback, int fd, const char *stack,
                     const char *address, const char *dns);
extern void stopTun(void);
extern void forceGC(void);
extern void updateDns(const char *dns);
/* 同步 invokeAction：解析 JSON Action 并按需以 result 回调返回 ActionResult */
extern void invokeAction(void *callback, const char *params);
extern void setEventListener(void *listener);
/* 返回 JSON：{"up":N,"down":N}，指针由内核持有一段静态缓冲，调用方应立即复制 */
extern char *getTotalTraffic(int only_statistics_proxy);
extern char *getTraffic(int only_statistics_proxy);
extern void suspend(int suspended);
extern void quickSetup(void *callback, const char *init_params,
                       const char *setup_params);

#ifdef __cplusplus
}
#endif