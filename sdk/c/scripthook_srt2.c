/*
 * gta-vi-scripthook: adapter for hosts that speak the Sonoma Runtime plugin API version 2.
 * SPDX-License-Identifier: MIT OR Apache-2.0
 *
 * Compile this file into a C or C++ plugin (it is plain C). It exports `srt_plugin_init` and
 * `srt_plugin_shutdown`, the entry points of Sonoma Runtime's plugin API version 2, builds an
 * ShkHost (scripthook.h) from the version-2 function table and calls the plugin's
 * `shk_plugin_load`. A host that speaks scripthook.h directly calls `shk_plugin_load` itself and
 * never calls these exports.
 *
 * What the version-2 host cannot provide:
 *   - host->draw is NULL (no drawing primitives for plugins yet);
 *   - host->input has version 1: no on_key, no capture;
 *   - host->services is emulated here: a small table in a process-local named shared memory
 *     section ("Local\gta-vi-scripthook-services-1-<pid>"), shared by every plugin built with
 *     this adapter or the Rust SDK (same layout). Entries are withdrawn when their plugin unloads.
 * See docs/host-integration.md for the host changes that remove the need for this file.
 */
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <string.h>
#include <stdio.h>
#include "scripthook.h"

/* ---- Sonoma Runtime plugin API version 2: the function table layout ------------------------ */
/* (docs/plugin-api.h of Sonoma Runtime; same order, same signatures. The structures it passes -
 * regions, modules, frame info, pads, values, native info, script status - have the same layout
 * as their Shk counterparts.) */
typedef struct Srt2Api {
    uint32_t size, version;
    void* log;
    void* notify;
    void* title_identity;
    void* mod_folder;
    void* read;
    void* write;
    void* patch;
    void* query;
    void* module_info;
    void* enum_modules;
    void* find_export;
    void* section;
    void* pattern_scan;
    void* pattern_scan_all;
    void* rip_target;
    void* resolve_chain;
    void* hook;
    void* unhook;
    void* on_frame;
    void* register_hotkey;
    void* register_pad_chord;
    void* key_down;
    void* pad_state;
    void* menu_page;
    void* menu_toggle;
    void* menu_slider;
    void* menu_number;
    void* menu_button;
    void* menu_text;
    void* menu_set_text;
    /* version 2 */
    void* native_lookup;
    void* native_call;
    void* hash_key;
    void* script_register;
    void* script_unregister;
    void* script_wait;
    void* script_status;
} Srt2Api;

#define SRT2_HAS(api, member) ((api)->size >= offsetof(Srt2Api, member) + sizeof(void*))

/* ---- the plugin's own entry points ------------------------------------------------------------ */
SHK_EXPORT int SHK_CALL shk_plugin_load(const ShkHost* host, ShkPlugin* self);
SHK_EXPORT void SHK_CALL shk_plugin_unload(void);
/* shk_plugin_unload is optional: without one the linker uses this */
SHK_EXTERN_C void SHK_CALL shk_srt2_default_unload(void) {}
#pragma comment(linker, "/alternatename:shk_plugin_unload=shk_srt2_default_unload")

/* ---- services: the shared table ------------------------------------------------------------ */
typedef struct ShkSvcEntry {
    char name[48];
    uint64_t iface;
    uint64_t owner;
} ShkSvcEntry;

#define SHK_SVC_MAGIC 0x53484B31u /* "SHK1" */
#define SHK_SVC_MAX 63

typedef struct ShkSvcTable {
    volatile LONG lock;
    uint32_t magic;
    uint32_t count;
    uint32_t reserved;
    ShkSvcEntry entries[SHK_SVC_MAX];
} ShkSvcTable;

static ShkSvcTable* g_svc;
static HANDLE g_svc_map;

static ShkSvcTable* svc_table(void) {
    if (g_svc) return g_svc;
    wchar_t name[96];
    swprintf(name, 96, L"Local\\gta-vi-scripthook-services-1-%lu", (unsigned long)GetCurrentProcessId());
    HANDLE m = CreateFileMappingW(INVALID_HANDLE_VALUE, NULL, PAGE_READWRITE, 0, 4096, name);
    if (!m) return NULL;
    ShkSvcTable* t = (ShkSvcTable*)MapViewOfFile(m, FILE_MAP_ALL_ACCESS, 0, 0, 4096);
    if (!t) {
        CloseHandle(m);
        return NULL;
    }
    g_svc_map = m; /* kept open while the plugin is loaded */
    g_svc = t;
    return t;
}

static void svc_lock(ShkSvcTable* t) {
    while (InterlockedCompareExchange(&t->lock, 1, 0) != 0) YieldProcessor();
    if (t->magic != SHK_SVC_MAGIC) { /* a fresh section is zero-filled */
        memset(t->entries, 0, sizeof t->entries);
        t->count = 0;
        t->magic = SHK_SVC_MAGIC;
    }
}

static void svc_unlock(ShkSvcTable* t) { InterlockedExchange(&t->lock, 0); }

static int SHK_CALL svc_publish(ShkPlugin* self, const char* name, const void* iface) {
    ShkSvcTable* t = svc_table();
    if (!t || !name || !*name || strlen(name) >= sizeof t->entries[0].name || !iface) return 0;
    svc_lock(t);
    int ok = 0;
    uint32_t i;
    for (i = 0; i < t->count; i++)
        if (strcmp(t->entries[i].name, name) == 0) break;
    if (i < t->count && t->entries[i].owner != (uint64_t)(uintptr_t)self) {
        ok = 0; /* another plugin owns the name */
    } else if (i < SHK_SVC_MAX) {
        memset(t->entries[i].name, 0, sizeof t->entries[i].name);
        memcpy(t->entries[i].name, name, strlen(name));
        t->entries[i].iface = (uint64_t)(uintptr_t)iface;
        t->entries[i].owner = (uint64_t)(uintptr_t)self;
        if (i == t->count) t->count++;
        ok = 1;
    }
    svc_unlock(t);
    return ok;
}

static void svc_remove_at(ShkSvcTable* t, uint32_t i) {
    t->entries[i] = t->entries[t->count - 1];
    memset(&t->entries[t->count - 1], 0, sizeof(ShkSvcEntry));
    t->count--;
}

static int SHK_CALL svc_withdraw(ShkPlugin* self, const char* name) {
    ShkSvcTable* t = svc_table();
    if (!t || !name) return 0;
    int ok = 0;
    svc_lock(t);
    for (uint32_t i = 0; i < t->count; i++) {
        if (strcmp(t->entries[i].name, name) == 0 && t->entries[i].owner == (uint64_t)(uintptr_t)self) {
            svc_remove_at(t, i);
            ok = 1;
            break;
        }
    }
    svc_unlock(t);
    return ok;
}

static const void* SHK_CALL svc_find(const char* name) {
    ShkSvcTable* t = svc_table();
    if (!t || !name) return NULL;
    const void* r = NULL;
    svc_lock(t);
    for (uint32_t i = 0; i < t->count; i++)
        if (strcmp(t->entries[i].name, name) == 0) {
            r = (const void*)(uintptr_t)t->entries[i].iface;
            break;
        }
    svc_unlock(t);
    return r;
}

static void svc_withdraw_all(ShkPlugin* self) {
    if (!g_svc) return;
    svc_lock(g_svc);
    for (uint32_t i = 0; i < g_svc->count;) {
        if (g_svc->entries[i].owner == (uint64_t)(uintptr_t)self) svc_remove_at(g_svc, i);
        else i++;
    }
    svc_unlock(g_svc);
}

/* ---- the host built from the version-2 table ------------------------------------------------ */
static ShkCoreApi g_core;
static ShkMemoryApi g_memory;
static ShkHookApi g_hooks;
static ShkInputApi g_input;
static ShkNativeApi g_natives;
static ShkScriptApi g_scripts;
static ShkHostMenuApi g_menu;
static ShkServiceApi g_services;
static ShkHost g_host;
static ShkPlugin* g_self;
static char g_version[64];

#define COPY(dst, src) (*(void**)&(dst) = (src))

SHK_EXPORT int SHK_CALL srt_plugin_init(const Srt2Api* api, ShkPlugin* self) {
    if (!api || api->version < 1) return 1;
    g_self = self;

    g_core.size = sizeof g_core; g_core.version = 1;
    COPY(g_core.log, api->log); COPY(g_core.notify, api->notify);
    COPY(g_core.title_identity, api->title_identity); COPY(g_core.plugin_folder, api->mod_folder);

    g_memory.size = sizeof g_memory; g_memory.version = 1;
    COPY(g_memory.read, api->read); COPY(g_memory.write, api->write); COPY(g_memory.patch, api->patch);
    COPY(g_memory.query, api->query); COPY(g_memory.module, api->module_info); COPY(g_memory.modules, api->enum_modules);
    COPY(g_memory.find_export, api->find_export); COPY(g_memory.section, api->section);
    COPY(g_memory.scan, api->pattern_scan); COPY(g_memory.scan_all, api->pattern_scan_all);
    COPY(g_memory.rip_target, api->rip_target); COPY(g_memory.resolve_chain, api->resolve_chain);

    g_hooks.size = sizeof g_hooks; g_hooks.version = 1;
    COPY(g_hooks.install, api->hook); COPY(g_hooks.remove, api->unhook);

    /* version 1 of the input group: up to `pad` */
    g_input.size = (uint32_t)offsetof(ShkInputApi, on_key); g_input.version = 1;
    COPY(g_input.on_frame, api->on_frame); COPY(g_input.hotkey, api->register_hotkey);
    COPY(g_input.pad_chord, api->register_pad_chord); COPY(g_input.key_down, api->key_down);
    COPY(g_input.pad, api->pad_state);

    g_menu.size = sizeof g_menu; g_menu.version = 1;
    COPY(g_menu.page, api->menu_page); COPY(g_menu.toggle, api->menu_toggle); COPY(g_menu.slider, api->menu_slider);
    COPY(g_menu.number, api->menu_number); COPY(g_menu.button, api->menu_button); COPY(g_menu.text, api->menu_text);
    COPY(g_menu.set_text, api->menu_set_text);

    g_services.size = sizeof g_services; g_services.version = 1;
    g_services.publish = svc_publish; g_services.withdraw = svc_withdraw; g_services.find = svc_find;

    g_host.size = sizeof g_host;
    g_host.abi_version = SHK_ABI_VERSION;
    g_host.host_name = "Sonoma Runtime";
    snprintf(g_version, sizeof g_version, "plugin API %u via the scripthook adapter", api->version);
    g_host.host_version = g_version;
    g_host.core = &g_core;
    g_host.memory = &g_memory;
    g_host.hooks = &g_hooks;
    g_host.input = &g_input;
    g_host.menu = &g_menu;
    g_host.draw = NULL;
    g_host.services = &g_services;
    if (api->version >= 2 && SRT2_HAS(api, script_status)) {
        g_natives.size = sizeof g_natives; g_natives.version = 1;
        COPY(g_natives.lookup, api->native_lookup); COPY(g_natives.call, api->native_call); COPY(g_natives.hash, api->hash_key);
        g_scripts.size = sizeof g_scripts; g_scripts.version = 1;
        COPY(g_scripts.start, api->script_register); COPY(g_scripts.stop, api->script_unregister);
        COPY(g_scripts.wait, api->script_wait); COPY(g_scripts.status, api->script_status);
        g_host.natives = &g_natives;
        g_host.scripts = &g_scripts;
    }
    int r = shk_plugin_load(&g_host, self);
    if (r != 0) svc_withdraw_all(self);
    return r;
}

SHK_EXPORT void SHK_CALL srt_plugin_shutdown(void) {
    shk_plugin_unload();
    svc_withdraw_all(g_self);
    if (g_svc) {
        UnmapViewOfFile(g_svc);
        g_svc = NULL;
    }
    if (g_svc_map) {
        CloseHandle(g_svc_map);
        g_svc_map = NULL;
    }
}
