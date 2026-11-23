// hello-cpp: the smallest gta-vi-scripthook plugin in C++ (scripthook.hpp).
// SPDX-License-Identifier: MIT OR Apache-2.0
//
// A notice (and a drawn box where the host can draw), typed native calls from a script
// function, and a hotkey (F8). Build: tools\build-examples.cmd (MSVC), with
// sdk\c\scripthook_srt2.c compiled in for Sonoma Runtime's current plugin API.
#include <atomic>
#include <cstdio>
#include "scripthook.hpp"

SHK_PLUGIN_INFO("hello-cpp", "0.1.0", "The gta-vi-scripthook contributors", "Hello world in C++: a notice, a native call, a hotkey.")

static std::atomic<bool> asked{false};

static void report(const char* what, bool notice) {
    int ped = shk::invoke<int>("PLAYER::PLAYER_PED_ID");
    shk::Vec3 p = shk::invoke<shk::Vec3>("ENTITY::GET_ENTITY_COORDS", ped, true);
    char b[160];
    std::snprintf(b, sizeof b, "hello-cpp: %s -> ped %d at (%.2f, %.2f, %.2f)", what, ped, p.x, p.y, p.z);
    shk::info("%s", b);
    if (notice) shk::notify(b);
}

SHK_EXPORT int SHK_CALL shk_plugin_load(const ShkHost* host, ShkPlugin* self) {
    shk::init(host, self);
    shk::info("hello-cpp: loaded on %s (%s)", host->host_name, host->host_version);
    shk::notify("Hello from C++ (F8: where am I?)", 4.0f);
    shk::on_draw(0, [](shk::Canvas& c) {
        float s = c.scale();
        float x = c.width() - 132 * s, y = 82 * s;
        c.rect(x, y, 120 * s, 18 * s, SHK_RGBA(12, 16, 22, 220));
        c.rect(x, y, 2 * s, 18 * s, SHK_RGBA(46, 196, 182, 255));
        c.text(x + 8 * s, y + 6 * s, "Hello from C++", SHK_RGBA(232, 237, 242, 255));
    });
    shk::on_hotkey(0x77 /* VK_F8 */, SHK_MOD_NONE, [] { asked = true; });
    shk::start_script("hello-cpp", [] {
        shk::Native ped_id("PLAYER::PLAYER_PED_ID");
        if (!ped_id.available()) {
            shk::warn("hello-cpp: %s", ped_id.info().reason);
            return;
        }
        report("native PLAYER_PED_ID", false);
        for (;;) {
            if (asked.exchange(false)) report("F8", true);
            shk::wait(0);
        }
    });
    return 0;
}

SHK_EXPORT void SHK_CALL shk_plugin_unload() {
    shk::info("hello-cpp: unloaded");
    shk::shutdown();
}
