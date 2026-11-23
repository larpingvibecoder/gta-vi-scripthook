/* hello-c: the smallest gta-vi-scripthook plugin in C.
 * SPDX-License-Identifier: MIT OR Apache-2.0
 *
 * - shows a notice when loaded (and draws a small box where the host can draw for plugins),
 * - calls natives from a script function on the game's script thread,
 * - registers a hotkey (F7) that asks the script for the player's position.
 *
 * Build: tools\build-examples.cmd (MSVC), with ..\..\sdk\c\scripthook_srt2.c compiled in so
 * that Sonoma Runtime's current plugin API (version 2) can load it too. */
#include <stdio.h>
#include <string.h>
#include "scripthook.h"

SHK_PLUGIN_INFO("hello-c", "0.1.0", "The gta-vi-scripthook contributors", "Hello world in C: a notice, a native call, a hotkey.")

static const ShkHost* host;
static ShkPlugin* me;
static volatile long asked; /* set by the hotkey, served by the script */

static void say(const char* text) { host->core->log(me, SHK_LOG_INFO, text); }

/* PLAYER::PLAYER_PED_ID() and ENTITY::GET_ENTITY_COORDS(ped, true) */
static int player_position(int32_t* ped, ShkVec3* pos) {
    ShkNative ped_id = host->natives->lookup("PLAYER::PLAYER_PED_ID", NULL);
    ShkNative coords = host->natives->lookup("ENTITY::GET_ENTITY_COORDS", NULL);
    ShkValue r;
    memset(&r, 0, sizeof r);
    r.type = SHK_TYPE_INT;
    if (!host->natives->call(ped_id, NULL, 0, &r)) return 0;
    *ped = r.u.i;
    ShkValue a[2];
    memset(a, 0, sizeof a);
    a[0].type = SHK_TYPE_INT;
    a[0].u.i = *ped;
    a[1].type = SHK_TYPE_BOOL;
    a[1].u.i = 1;
    memset(&r, 0, sizeof r);
    r.type = SHK_TYPE_VEC3;
    if (!host->natives->call(coords, a, 2, &r)) return 0;
    *pos = r.u.v;
    return 1;
}

static void SHK_CALL script_main(void* user) {
    (void)user;
    char b[160];
    int32_t ped;
    ShkVec3 p;
    if (player_position(&ped, &p)) {
        snprintf(b, sizeof b, "hello-c: native PLAYER_PED_ID -> %d at (%.2f, %.2f, %.2f)", ped, p.x, p.y, p.z);
        say(b);
    } else {
        say("hello-c: natives not available yet (no natives database for this game)");
    }
    for (;;) {
        if (asked) {
            asked = 0;
            if (player_position(&ped, &p)) {
                snprintf(b, sizeof b, "hello-c: F7 -> ped %d at (%.1f, %.1f, %.1f)", ped, p.x, p.y, p.z);
                host->core->notify(me, b, 3.0f);
                say(b);
            }
        }
        host->scripts->wait(0); /* next script tick */
    }
}

static void SHK_CALL on_f7(void* user) {
    (void)user;
    asked = 1;
}

static void SHK_CALL draw(void* user, const ShkCanvas* c) {
    (void)user;
    float s = c->ui_scale;
    float x = c->width - 132 * s, y = 60 * s;
    c->rect(c->ctx, x, y, 120 * s, 18 * s, SHK_RGBA(12, 16, 22, 220));
    c->rect(c->ctx, x, y, 2 * s, 18 * s, SHK_RGBA(46, 196, 182, 255));
    c->text(c->ctx, x + 8 * s, y + 6 * s, s, SHK_RGBA(232, 237, 242, 255), "Hello from C");
}

SHK_EXPORT int SHK_CALL shk_plugin_load(const ShkHost* h, ShkPlugin* self) {
    host = h;
    me = self;
    char b[200];
    snprintf(b, sizeof b, "hello-c: loaded on %s (%s)", h->host_name, h->host_version);
    say(b);
    h->core->notify(self, "Hello from C (F7: where am I?)", 4.0f);
    if (h->draw) h->draw->on_draw(self, 0, draw, NULL);
    h->input->hotkey(self, 0x76 /* VK_F7 */, SHK_MOD_NONE, on_f7, NULL);
    if (h->scripts && h->natives) h->scripts->start(self, "hello-c", script_main, NULL);
    return 0;
}

SHK_EXPORT void SHK_CALL shk_plugin_unload(void) { say("hello-c: unloaded"); }
