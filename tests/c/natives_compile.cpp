// Compile check of a generated natives.h (C++). SPDX-License-Identifier: MIT OR Apache-2.0
#include "natives.h"

void use_generated_natives() {
    natives::Ped ped = natives::PLAYER::PLAYER_PED_ID();
    shk::Vec3 p = natives::ENTITY::GET_ENTITY_COORDS(ped, true);
    natives::ENTITY::SET_ENTITY_HEADING(ped, p.x);
    int32_t cash = 0;
    natives::STATS::STAT_GET_INT(shk::Hash("SP0_TOTAL_CASH"), &cash, -1);
    natives::MISC::SET_WEATHER_TYPE_NOW_PERSIST("RAIN");
}
