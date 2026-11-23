/* Compile check of a generated natives_c.h (C). SPDX-License-Identifier: MIT OR Apache-2.0 */
#include "natives_c.h"

const ShkHost* shk_host;

void use_generated_natives(void) {
    int32_t ped = PLAYER__PLAYER_PED_ID();
    ShkVec3 p = ENTITY__GET_ENTITY_COORDS(ped, 1);
    ENTITY__SET_ENTITY_HEADING(ped, p.x);
    MISC__SET_WEATHER_TYPE_NOW_PERSIST("RAIN");
}
