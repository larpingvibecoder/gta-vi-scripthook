/*
 * gta-vi-scripthook plugin SDK: the C ABI (scripthook.h), ABI version 1.
 * SPDX-License-Identifier: MIT OR Apache-2.0
 *
 * Unofficial, single-player / offline only. Not affiliated with or endorsed by Rockstar Games
 * or Take-Two Interactive. Nothing in this ABI touches online features or protection.
 *
 * ---------------------------------------------------------------------------------------------
 * What a plugin is
 * ---------------------------------------------------------------------------------------------
 * A plugin is a native x64 Windows DLL in the `plugins` folder of gta-vi-scripthook's hook
 * (scripthook.dll), named by its folder's plugin.toml / mod.toml (`plugin = "my_plugin.dll"`).
 * The hook - running inside the game process - loads it at start and serves it the tables below:
 * memory, modules and patterns, hooks, input, natives (the game's script commands), script fibers
 * on the game's script thread, drawing, and services shared between plugins. A plugin only ever
 * talks to the game through these tables.
 *
 * Exports (SHK_EXPORT is __declspec(dllexport) + C linkage):
 *
 *     int  shk_plugin_load(const ShkHost* host, ShkPlugin* self);   required; 0 = loaded
 *     void shk_plugin_unload(void);                                  optional
 *     const ShkPluginInfo* shk_plugin_info(void);                    optional (SHK_PLUGIN_INFO)
 *
 * Optionally, runtimes that only speak Sonoma Runtime's plugin API version 2 (`srt_plugin_init`,
 * docs/host-integration.md) can load the plugin directly through the adapter `sdk/c/scripthook_srt2.c`
 * (C / C++) or the Rust SDK's built-in adapter: compile it into the plugin and it exports
 * `srt_plugin_init` / `srt_plugin_shutdown`, builds an ShkHost from the version-2 table and calls
 * shk_plugin_load. Groups the old host cannot provide are NULL (`draw`) or have a smaller
 * `size` (members appended later are missing) - always check before use:
 *
 *     if (host->draw) ...                                    a group may be absent
 *     if (SHK_HAS(host->input, on_key)) ...                  a member may be absent
 *
 * ---------------------------------------------------------------------------------------------
 * Versioning
 * ---------------------------------------------------------------------------------------------
 * ShkHost.abi_version is SHK_ABI_VERSION of the host. Every group table starts with
 * { uint32_t size; uint32_t version; }. Members are only ever appended to a group: a newer host
 * keeps every member at its offset, so test `size` (SHK_HAS) rather than equality. A breaking
 * change would bump SHK_ABI_VERSION and the export names.
 *
 * ---------------------------------------------------------------------------------------------
 * Threads
 * ---------------------------------------------------------------------------------------------
 * - shk_plugin_load / shk_plugin_unload: the host's main thread.
 * - Frame, draw, hotkey, pad-chord, key and menu callbacks: the thread that presents frames,
 *   before the frame is presented, one at a time.
 * - Script functions (scripts->start): the game's script thread, each on its own fiber, once per
 *   script tick until they call scripts->wait; or, while the game's script tick is unknown,
 *   before each presented frame (scripts->status says which). Natives can only be called from a
 *   script function.
 * - memory, modules, patterns, natives->lookup, hash: any thread.
 *
 * Everything a plugin registered (callbacks, hooks, scripts, menu pages, services) is removed
 * when it is unloaded: scripts first (after a script tick in progress has finished), then hooks
 * are restored, then the DLL is freed. A suspended script is never resumed and no destructors
 * run on its fiber stack.
 *
 * Return conventions: functions returning `int` return 1 on success and 0 on failure (the
 * reason goes to the host log) unless documented otherwise; handles are 0 when invalid.
 * Strings are NUL-terminated UTF-8. Addresses are title virtual addresses (the title runs in the
 * host's process); always go through memory->read / write: they are bounds-checked and never
 * fault.
 */
#ifndef SCRIPTHOOK_H
#define SCRIPTHOOK_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define SHK_ABI_VERSION 1u
#define SHK_SDK_VERSION_STRING "0.1.0"

#define SHK_CALL __cdecl /* x64 has one calling convention; spelled out for clarity */
#ifdef __cplusplus
#define SHK_EXTERN_C extern "C"
#else
#define SHK_EXTERN_C
#endif
#define SHK_EXPORT SHK_EXTERN_C __declspec(dllexport)

/* true when the group `table` is present and its `size` covers `member` */
#define SHK_HAS(table, member) \
    ((table) != NULL && (table)->size >= (uint32_t)((const char*)&(table)->member - (const char*)(table)) + sizeof((table)->member))

/* ============================================================================================ */
/* Basic types                                                                                   */
/* ============================================================================================ */

typedef struct ShkPlugin ShkPlugin; /* opaque: identifies the plugin in registering calls */
typedef uint64_t ShkAddr;           /* a title virtual address */
typedef uint64_t ShkHook;           /* inline hook handle, 0 = none */
typedef uint64_t ShkNative;         /* native handle from natives->lookup, 0 = unknown */
typedef int32_t ShkScript;          /* script function id, > 0 */

enum ShkLogLevel { SHK_LOG_ERROR = 0, SHK_LOG_WARN = 1, SHK_LOG_INFO = 2, SHK_LOG_DEBUG = 3 };

typedef struct ShkVec3 {
    float x, y, z;
} ShkVec3;

/* A memory region (VirtualQuery). 40 bytes. */
typedef struct ShkRegion {
    ShkAddr base;
    uint64_t size;
    uint32_t protect;  /* PAGE_* */
    uint32_t state;    /* MEM_COMMIT / MEM_RESERVE / MEM_FREE */
    uint32_t type;     /* MEM_PRIVATE / MEM_MAPPED / MEM_IMAGE */
    uint32_t readable; /* 1 if memory->read can read it */
    uint32_t writable; /* 1 if memory->write can write it */
    uint32_t reserved;
} ShkRegion;

/* A guest module: the title image or one of its DLLs. 88 bytes. */
typedef struct ShkModule {
    ShkAddr base;
    uint64_t size;
    char name[64];     /* file name */
    uint32_t is_title; /* 1 for the title executable */
    uint32_t sections; /* number of PE sections */
} ShkModule;

/* Passed to frame callbacks. 32 bytes. */
typedef struct ShkFrameInfo {
    uint64_t frame;     /* present index */
    double dt;          /* seconds since the previous frame callback (0 .. 0.25) */
    uint32_t width;     /* presented image size in pixels */
    uint32_t height;
    uint32_t menu_open; /* 1 while the host's own menu is open (the title gets no input then) */
    uint32_t reserved;
} ShkFrameInfo;

/* Gamepad state: GameInput button bits, triggers 0..1, sticks -1..1. 28 bytes. */
typedef struct ShkPad {
    uint32_t buttons;
    float left_trigger, right_trigger;
    float left_x, left_y, right_x, right_y;
} ShkPad;

enum ShkPadButton {
    SHK_PAD_MENU = 0x1, SHK_PAD_VIEW = 0x2, SHK_PAD_A = 0x4, SHK_PAD_B = 0x8, SHK_PAD_X = 0x10,
    SHK_PAD_Y = 0x20, SHK_PAD_UP = 0x40, SHK_PAD_DOWN = 0x80, SHK_PAD_LEFT = 0x100,
    SHK_PAD_RIGHT = 0x200, SHK_PAD_LB = 0x400, SHK_PAD_RB = 0x800, SHK_PAD_LS = 0x1000,
    SHK_PAD_RS = 0x2000
};

/* Hotkey modifiers */
enum ShkKeyMod { SHK_MOD_NONE = 0, SHK_MOD_SHIFT = 1, SHK_MOD_CTRL = 2, SHK_MOD_ALT = 4 };

/* A keyboard event (input->on_key). 16 bytes. */
typedef struct ShkKeyEvent {
    uint32_t vk;     /* Windows virtual-key code, 0 for a pure character event */
    uint32_t down;   /* 1 = pressed, 0 = released */
    uint32_t repeat; /* 1 = auto-repeat */
    uint32_t ch;     /* typed character (UTF-32), 0 if none */
} ShkKeyEvent;

typedef void (SHK_CALL *ShkFrameFn)(void* user, const ShkFrameInfo* info);
typedef void (SHK_CALL *ShkEventFn)(void* user);
typedef void (SHK_CALL *ShkModuleFn)(void* user, const ShkModule* module);
typedef void (SHK_CALL *ShkScriptFn)(void* user);
/* returns 1 to consume the key (not seen by the title or later handlers) */
typedef int (SHK_CALL *ShkKeyFn)(void* user, const ShkKeyEvent* ev);
/* host basic-menu item changed; `value` points at the item's variable */
typedef void (SHK_CALL *ShkChangedFn)(void* user, const void* value);

/* ============================================================================================ */
/* Natives                                                                                       */
/* ============================================================================================ */

/* Value types of native arguments and results. INT is 32-bit (handles: Ped, Vehicle, Entity,
 * Player, ...); HASH is a 32-bit name hash (see natives->hash); STRING is `const char*`; VEC3 is
 * three floats (three argument slots); OUT_* point at the plugin's int32_t / float / int32_t /
 * ShkVec3 that the native fills (copied back after the call, vector fix-ups included). */
typedef enum ShkType {
    SHK_TYPE_VOID = 0, SHK_TYPE_INT = 1, SHK_TYPE_FLOAT = 2, SHK_TYPE_BOOL = 3, SHK_TYPE_HASH = 4,
    SHK_TYPE_STRING = 5, SHK_TYPE_VEC3 = 6, SHK_TYPE_U64 = 7, SHK_TYPE_PTR = 8,
    SHK_TYPE_OUT_INT = 9, SHK_TYPE_OUT_FLOAT = 10, SHK_TYPE_OUT_BOOL = 11, SHK_TYPE_OUT_VEC3 = 12,
    SHK_TYPE_UNKNOWN = 0xFFFFFFFFu
} ShkType;

/* A typed value, 24 bytes. Arguments are converted to the types the natives database declares
 * (an INT for a FLOAT argument is converted; a STRING for a HASH argument is hashed; a VEC3
 * fills three float arguments). For a result set `type` to the type you expect (used when the
 * database does not declare it); on return it holds the type decoded. A STRING result stays
 * valid until the next call on the same thread. */
typedef struct ShkValue {
    uint32_t type; /* ShkType */
    uint32_t reserved;
    union {
        int32_t i;     /* INT, BOOL (0 / 1) */
        uint32_t hash; /* HASH */
        float f;       /* FLOAT */
        const char* s; /* STRING */
        ShkVec3 v;     /* VEC3 */
        uint64_t u64;  /* U64 */
        void* p;       /* PTR, OUT_* (the plugin's storage) */
        uint64_t raw[2];
    } u;
} ShkValue;

/* What the natives database knows about a native. 408 bytes. */
typedef struct ShkNativeInfo {
    uint64_t id;            /* the game's identifier (hash or index), 0 = unknown */
    uint32_t available;     /* 1 = callable now */
    uint32_t arg_count;     /* declared arguments, 0xFFFFFFFF = not declared */
    uint32_t arg_types[32]; /* ShkType of each declared argument */
    uint32_t ret_type;      /* ShkType, 0xFFFFFFFF = not declared */
    uint32_t reserved;
    char name[96];          /* "NAMESPACE::NAME" */
    char reason[160];       /* why it is not available, e.g. "needs native X (unknown until the executable)" */
} ShkNativeInfo;

typedef struct ShkNativeApi {
    uint32_t size, version;
    /* by name: "NAMESPACE::NAME", "NAME", or "0x<hash>" for a native the database lacks.
     * 0 if the database does not know it; `info` (may be NULL) is filled either way. A handle
     * can be returned for a native that is not callable yet (info->available == 0). */
    ShkNative (SHK_CALL *lookup)(const char* name, ShkNativeInfo* info);
    /* call from a script function; `result` may be NULL. 1 = done, 0 = failed (unknown native,
     * not on a script fiber, wrong argument count or type; reason in the host log) */
    int (SHK_CALL *call)(ShkNative native, const ShkValue* args, uint32_t count, ShkValue* result);
    /* the engine's 32-bit name hash: Jenkins one-at-a-time of the lower-cased text */
    uint32_t (SHK_CALL *hash)(const char* text);
} ShkNativeApi;

/* ============================================================================================ */
/* Script functions                                                                              */
/* ============================================================================================ */

enum ShkScriptMode { SHK_SCRIPTS_OFF = 0, SHK_SCRIPTS_GAME_THREAD = 1, SHK_SCRIPTS_PRESENT_FALLBACK = 2 };

/* 184 bytes */
typedef struct ShkScriptStatus {
    uint32_t mode;        /* ShkScriptMode */
    uint32_t scripts;     /* registered script functions (all plugins) */
    uint64_t ticks;       /* script ticks so far */
    uint32_t tick_thread; /* thread id of the last tick */
    uint32_t in_script;   /* 1 if the caller is a script function */
    char reason[160];     /* why the present-time fallback is used */
} ShkScriptStatus;

typedef struct ShkScriptApi {
    uint32_t size, version;
    /* a script function: runs from the next script tick on its own fiber; returning ends it.
     * Returns its id (> 0) or 0. */
    ShkScript (SHK_CALL *start)(ShkPlugin* self, const char* name, ShkScriptFn fn, void* user);
    /* removes one of the plugin's scripts; called by the script itself it stops at its next
     * wait. 1 if removed. */
    int (SHK_CALL *stop)(ShkPlugin* self, ShkScript script);
    /* inside a script: yield, return after at least `ms` milliseconds (0 = next tick), 1.
     * Outside a script: returns 0 at once (never blocks). */
    int (SHK_CALL *wait)(uint32_t ms);
    /* ShkScriptMode; fills `out` when not NULL */
    int (SHK_CALL *status)(ShkScriptStatus* out);
} ShkScriptApi;

/* ============================================================================================ */
/* Core, memory, hooks, input                                                                    */
/* ============================================================================================ */

typedef struct ShkCoreApi {
    uint32_t size, version;
    void (SHK_CALL *log)(ShkPlugin* self, int level, const char* text);
    /* an on-screen notice for `seconds` (shown while menus are closed too) */
    void (SHK_CALL *notify)(ShkPlugin* self, const char* text, float seconds);
    /* the title's identity, e.g. "Game.28872443D20A7" */
    const char* (SHK_CALL *title_identity)(void);
    /* the plugin's mod folder (where its DLL and data files are) */
    const char* (SHK_CALL *plugin_folder)(ShkPlugin* self);
} ShkCoreApi;

typedef struct ShkMemoryApi {
    uint32_t size, version;
    int (SHK_CALL *read)(ShkAddr addr, void* out, size_t len);
    /* writable title memory only (not code, not system images) */
    int (SHK_CALL *write)(ShkAddr addr, const void* data, size_t len);
    /* code / read-only data of guest modules: protection lifted for the write, i-cache flushed */
    int (SHK_CALL *patch)(ShkAddr addr, const void* data, size_t len);
    int (SHK_CALL *query)(ShkAddr addr, ShkRegion* out);
    /* module: "game.exe", "game", or NULL / "" for the title image; 1 if found */
    int (SHK_CALL *module)(const char* name, ShkModule* out);
    /* calls `fn` for every guest module, title image first; returns the count */
    int (SHK_CALL *modules)(ShkModuleFn fn, void* user);
    ShkAddr (SHK_CALL *find_export)(const char* module, const char* symbol);
    int (SHK_CALL *section)(const char* module, const char* name, ShkAddr* start, uint64_t* size);
    /* pattern: hex bytes, "?" / "??" any byte. section NULL = every section. n-th match, 0 if none */
    ShkAddr (SHK_CALL *scan)(const char* module, const char* section, const char* pattern, uint32_t nth);
    size_t (SHK_CALL *scan_all)(const char* module, const char* section, const char* pattern, ShkAddr* out, size_t max);
    /* insn + insn_len + *(int32*)(insn + disp_offset); 0 if unreadable */
    ShkAddr (SHK_CALL *rip_target)(ShkAddr insn, uint32_t disp_offset, uint32_t insn_len);
    /* a = base; for each offset: a = *(uint64*)a + offset; 0 when a link is unreadable or null */
    ShkAddr (SHK_CALL *resolve_chain)(ShkAddr base, const int64_t* offsets, size_t count);
} ShkMemoryApi;

typedef struct ShkHookApi {
    uint32_t size, version;
    /* inline hook of the title function at `target` (inside a guest module); `detour` has the
     * target's signature; `*original` receives a callable trampoline. 0 on failure. */
    ShkHook (SHK_CALL *install)(ShkPlugin* self, ShkAddr target, void* detour, void** original);
    int (SHK_CALL *remove)(ShkPlugin* self, ShkHook hook);
} ShkHookApi;

typedef struct ShkInputApi {
    uint32_t size, version;
    /* once per presented frame, before it is presented */
    int (SHK_CALL *on_frame)(ShkPlugin* self, ShkFrameFn fn, void* user);
    /* virtual-key code + ShkKeyMod bits; fires on key down while the game window has the focus
     * and no menu is open */
    int (SHK_CALL *hotkey)(ShkPlugin* self, uint32_t vk, uint32_t mods, ShkEventFn fn, void* user);
    /* fires when all `buttons` (ShkPadButton) become held on any pad */
    int (SHK_CALL *pad_chord)(ShkPlugin* self, uint32_t buttons, ShkEventFn fn, void* user);
    /* current input, not blocked by menus: 1 if held / connected */
    int (SHK_CALL *key_down)(uint32_t vk);
    int (SHK_CALL *pad)(uint32_t user, ShkPad* out);
    /* ---- version 2 (not offered by Sonoma Runtime API v2 hosts) ---- */
    /* every keyboard event of the game window, frame thread, before hotkeys */
    int (SHK_CALL *on_key)(ShkPlugin* self, ShkKeyFn fn, void* user);
    /* while captured, the title gets no keyboard / pad input and the host's own menu key and
     * hotkeys are suspended (a plugin menu is open). Returns 1. */
    int (SHK_CALL *capture)(ShkPlugin* self, int on);
} ShkInputApi;

/* ============================================================================================ */
/* Host basic menu: pages of simple items drawn by the host (Sonoma Runtime's own menu)          */
/* ============================================================================================ */

typedef uint32_t ShkHostPage; /* 0 = none */

typedef struct ShkHostMenuApi {
    uint32_t size, version;
    ShkHostPage (SHK_CALL *page)(ShkPlugin* self, const char* title);
    /* `value` is the plugin's variable, read and written by the menu on the frame thread; item
     * functions return the item index or -1 */
    int (SHK_CALL *toggle)(ShkPlugin* self, ShkHostPage page, const char* label, int32_t* value, ShkChangedFn changed, void* user);
    int (SHK_CALL *slider)(ShkPlugin* self, ShkHostPage page, const char* label, float* value, float min, float max, float step, ShkChangedFn changed, void* user);
    int (SHK_CALL *number)(ShkPlugin* self, ShkHostPage page, const char* label, double* value, ShkChangedFn changed, void* user);
    int (SHK_CALL *button)(ShkPlugin* self, ShkHostPage page, const char* label, ShkEventFn pressed, void* user);
    int (SHK_CALL *text)(ShkPlugin* self, ShkHostPage page, const char* text);
    int (SHK_CALL *set_text)(ShkPlugin* self, ShkHostPage page, int item, const char* text);
} ShkHostMenuApi;

/* ============================================================================================ */
/* Drawing (overlay on the presented image)                                                      */
/* ============================================================================================ */

/* Colours are 0xAARRGGBB. */
#define SHK_RGBA(r, g, b, a) ((uint32_t)(a) << 24 | (uint32_t)(r) << 16 | (uint32_t)(g) << 8 | (uint32_t)(b))

/* A canvas for one frame, valid during the draw callback. Pixels, origin top left. Text uses
 * the overlay's font (Inter): at `scale` s its pixel size is round(8.5 * s); `text` draws with
 * the cap line at y and returns the width; `measure` gives the width without drawing. */
typedef struct ShkCanvas {
    uint32_t size;
    uint32_t width, height;    /* presented image */
    float ui_scale;            /* suggested scale: 1 below 600 lines, 2 up to 1300, 3 above */
    float cap_height;          /* cap height of text at scale 1, pixels */
    float line_height;         /* recommended line height at scale 1, pixels */
    uint32_t reserved[2];
    void* ctx;
    void (SHK_CALL *rect)(void* ctx, float x, float y, float w, float h, uint32_t argb);
    /* ASCII 32..126; other characters draw as '?'; returns the width in pixels */
    float (SHK_CALL *text)(void* ctx, float x, float y, float scale, uint32_t argb, const char* text);
    /* subsequent drawing is clipped to the rectangle; w <= 0 or h <= 0 removes the clip */
    void (SHK_CALL *clip)(void* ctx, float x, float y, float w, float h);
    float (SHK_CALL *measure)(void* ctx, float scale, const char* text);
} ShkCanvas;

typedef void (SHK_CALL *ShkDrawFn)(void* user, const ShkCanvas* canvas);

typedef struct ShkDrawApi {
    uint32_t size, version;
    /* called once per presented frame on the frame thread, after the frame callbacks; layer
     * 0 = HUD (below), 1 = menu, 2 = notices (top) */
    int (SHK_CALL *on_draw)(ShkPlugin* self, uint32_t layer, ShkDrawFn fn, void* user);
} ShkDrawApi;

/* ============================================================================================ */
/* Services: named interfaces one plugin publishes and others use                                */
/* ============================================================================================ */

typedef struct ShkServiceApi {
    uint32_t size, version;
    /* `iface` stays valid until withdrawn or the publisher is unloaded (withdrawn then) */
    int (SHK_CALL *publish)(ShkPlugin* self, const char* name, const void* iface);
    int (SHK_CALL *withdraw)(ShkPlugin* self, const char* name);
    /* the interface or NULL. Look it up again when you use it later: the publisher may have
     * been unloaded. */
    const void* (SHK_CALL *find)(const char* name);
} ShkServiceApi;

/* ============================================================================================ */
/* The host                                                                                      */
/* ============================================================================================ */

typedef struct ShkHost {
    uint32_t size;
    uint32_t abi_version;       /* SHK_ABI_VERSION of the host */
    const char* host_name;      /* e.g. "Sonoma Runtime" */
    const char* host_version;   /* e.g. "0.1.0 (plugin API 2 via adapter)" */
    const ShkCoreApi* core;
    const ShkMemoryApi* memory;
    const ShkHookApi* hooks;
    const ShkInputApi* input;
    const ShkNativeApi* natives;
    const ShkScriptApi* scripts;
    const ShkHostMenuApi* menu; /* the host's basic menu; NULL if none */
    const ShkDrawApi* draw;     /* NULL if the host cannot draw for plugins */
    const ShkServiceApi* services;
} ShkHost;

/* Optional export describing the plugin (shown by hosts and the menu). */
typedef struct ShkPluginInfo {
    uint32_t size;
    uint32_t abi_version;    /* SHK_ABI_VERSION the plugin was built with */
    const char* name;
    const char* version;
    const char* author;
    const char* description;
} ShkPluginInfo;

#define SHK_PLUGIN_INFO(name_, version_, author_, description_)                              \
    SHK_EXPORT const ShkPluginInfo* SHK_CALL shk_plugin_info(void) {                         \
        static const ShkPluginInfo info_ = {sizeof(ShkPluginInfo), SHK_ABI_VERSION, name_,   \
                                            version_, author_, description_};                \
        return &info_;                                                                       \
    }

typedef int (SHK_CALL *ShkPluginLoadFn)(const ShkHost* host, ShkPlugin* self);
typedef void (SHK_CALL *ShkPluginUnloadFn)(void);
typedef const ShkPluginInfo* (SHK_CALL *ShkPluginInfoFn)(void);

/* ============================================================================================ */
/* The menu service ("scripthook.menu", published by the gta-vi-scripthook menu plugin)          */
/* ============================================================================================ */
/* A richer menu than the host's basic one: nested pages, choices, text entry, disabled items
 * with a reason, hints, HUD lines. Find it with services->find(SHK_MENU_SERVICE) (after load:
 * at the first frame callback every plugin is loaded). Calls may come from any thread; events
 * arrive on the frame thread. */

#define SHK_MENU_SERVICE "scripthook.menu"

typedef uint32_t ShkMenuPage; /* 0 = the root page */
typedef uint32_t ShkMenuItem; /* 0 = none */

typedef enum ShkItemKind {
    SHK_ITEM_TEXT = 0,      /* a line of text (selectable when it has a hint) */
    SHK_ITEM_BUTTON = 1,    /* Enter / A: ACTIVATE */
    SHK_ITEM_TOGGLE = 2,    /* value 0 / 1: CHANGE */
    SHK_ITEM_SLIDER = 3,    /* value in [min, max] by step: CHANGE; Enter types a number */
    SHK_ITEM_NUMBER = 4,    /* unbounded value: CHANGE */
    SHK_ITEM_CHOICE = 5,    /* value = option index: CHANGE */
    SHK_ITEM_INPUT = 6,     /* text entry: TEXT */
    SHK_ITEM_SUBPAGE = 7,   /* opens `target` */
    SHK_ITEM_SEPARATOR = 8  /* a labelled divider */
} ShkItemKind;

enum ShkItemFlag {
    SHK_ITEMF_DISABLED = 1,  /* greyed; `reason` says why */
    SHK_ITEMF_FILTERABLE = 2 /* SUBPAGE: the opened page gets a filter row */
};

enum ShkMenuEventType { SHK_EVENT_ACTIVATE = 1, SHK_EVENT_CHANGE = 2, SHK_EVENT_TEXT = 3 };

typedef struct ShkMenuEvent {
    uint32_t size;
    uint32_t type;    /* ShkMenuEventType */
    ShkMenuItem item;
    uint32_t reserved;
    double value;     /* CHANGE: the new value */
    const char* text; /* TEXT: the typed text (valid during the callback) */
} ShkMenuEvent;

typedef void (SHK_CALL *ShkMenuEventFn)(void* user, const ShkMenuEvent* ev);

typedef struct ShkMenuItemDesc {
    uint32_t size;          /* sizeof(ShkMenuItemDesc) */
    uint32_t kind;          /* ShkItemKind */
    uint32_t flags;         /* ShkItemFlag */
    uint32_t option_count;  /* CHOICE */
    const char* label;
    const char* hint;       /* shown in the description panel while selected; may be NULL */
    const char* reason;     /* why it is disabled; may be NULL */
    const char* right;      /* text drawn on the right instead of the kind's own; may be NULL */
    const char* const* options; /* CHOICE */
    const char* text;       /* INPUT: initial text */
    double value, min, max, step;
    ShkMenuPage target;     /* SUBPAGE */
    uint32_t hotkey;        /* virtual key shown next to the label (informational), 0 = none */
    ShkMenuEventFn on_event; /* may be NULL */
    void* user;
} ShkMenuItemDesc;

typedef struct ShkMenuService {
    uint32_t size, version;
    /* a page; parent 0 = listed on the root page. Returns the page id (> 0) or 0. */
    ShkMenuPage (SHK_CALL *add_page)(ShkPlugin* owner, ShkMenuPage parent, const char* title, const char* subtitle);
    ShkMenuItem (SHK_CALL *add_item)(ShkPlugin* owner, ShkMenuPage page, const ShkMenuItemDesc* desc);
    int (SHK_CALL *set_value)(ShkMenuItem item, double value);
    /* NULL leaves a field unchanged */
    int (SHK_CALL *set_text)(ShkMenuItem item, const char* label, const char* right, const char* hint);
    int (SHK_CALL *set_enabled)(ShkMenuItem item, int enabled, const char* reason);
    /* removes every item of the page (dynamic lists) */
    int (SHK_CALL *clear_page)(ShkMenuPage page);
    /* removes everything the owner added; call it from shk_plugin_unload */
    int (SHK_CALL *remove_owner)(ShkPlugin* owner);
    /* a HUD line at the top right while non-empty; slot 0..7 per owner */
    int (SHK_CALL *set_hud)(ShkPlugin* owner, uint32_t slot, const char* text);
    /* 1 while the menu is open */
    int (SHK_CALL *is_open)(void);
    /* a notice through the menu's toast stack (falls back to core->notify) */
    int (SHK_CALL *toast)(ShkPlugin* owner, const char* text, float seconds);
} ShkMenuService;

#ifdef __cplusplus
}
#endif

#endif /* SCRIPTHOOK_H */
