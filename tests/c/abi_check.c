/* Prints the sizes and member offsets of scripthook.h's structures; tests/tests/abi.rs compares
 * them with the Rust SDK's declarations. SPDX-License-Identifier: MIT OR Apache-2.0 */
#include <stdio.h>
#include <stddef.h>
#include "scripthook.h"

#define S(t) printf("size %s %u\n", #t, (unsigned)sizeof(t))
#define O(t, m) printf("offset %s.%s %u\n", #t, #m, (unsigned)offsetof(t, m))

int main(void) {
    S(ShkVec3); S(ShkRegion); S(ShkModule); S(ShkFrameInfo); S(ShkPad); S(ShkKeyEvent);
    S(ShkValue); S(ShkNativeInfo); S(ShkScriptStatus); S(ShkCanvas); S(ShkPluginInfo);
    S(ShkMenuEvent); S(ShkMenuItemDesc);
    S(ShkCoreApi); S(ShkMemoryApi); S(ShkHookApi); S(ShkInputApi); S(ShkNativeApi); S(ShkScriptApi);
    S(ShkHostMenuApi); S(ShkDrawApi); S(ShkServiceApi); S(ShkHost); S(ShkMenuService);
    O(ShkValue, u); O(ShkNativeInfo, ret_type); O(ShkNativeInfo, name); O(ShkNativeInfo, reason);
    O(ShkScriptStatus, reason); O(ShkCanvas, ctx); O(ShkCanvas, clip);
    O(ShkInputApi, on_key); O(ShkInputApi, capture); O(ShkHost, services); O(ShkHost, draw);
    O(ShkMenuItemDesc, value); O(ShkMenuItemDesc, target); O(ShkMenuItemDesc, on_event); O(ShkMenuItemDesc, user);
    O(ShkMenuService, toast); O(ShkMenuEvent, value); O(ShkMenuEvent, text);
    return 0;
}
