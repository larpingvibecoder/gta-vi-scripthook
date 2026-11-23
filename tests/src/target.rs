//! The synthetic test target (`shk-test-target`, tests/target/main.rs): the natives it registers
//! and the `natives.tsv` describing them. The target is this repository's own stand-in for a
//! game: a natives table found by a byte pattern (8 chained buckets, XOR-obfuscated hashes,
//! the default call-context layout), a script thread calling a tick function, a player record,
//! and a D3D12 window that presents every frame.

/// (name, argument types, result type)
pub const NATIVES: &[(&str, &str, &str)] = &[
    ("PLAYER::PLAYER_ID", "0", "int"),
    ("PLAYER::PLAYER_PED_ID", "0", "Ped"),
    ("PED::GET_VEHICLE_PED_IS_IN", "Ped, BOOL", "Vehicle"),
    ("PLAYER::SET_PLAYER_INVINCIBLE", "Player, BOOL", "void"),
    ("ENTITY::GET_ENTITY_COORDS", "Entity, BOOL", "Vector3"),
    ("ENTITY::SET_ENTITY_COORDS_NO_OFFSET", "7", "0"),
    ("ENTITY::GET_ENTITY_HEADING", "Entity", "float"),
    ("ENTITY::SET_ENTITY_HEADING", "Entity, float", "void"),
    ("ENTITY::SET_ENTITY_VELOCITY", "Entity, float, float, float", "void"),
    ("CAM::GET_GAMEPLAY_CAM_ROT", "int", "Vector3"),
    ("ENTITY::GET_OFFSET_FROM_ENTITY_IN_WORLD_COORDS", "Entity, float, float, float", "Vector3"),
    ("STREAMING::REQUEST_MODEL", "Hash", "void"),
    ("STREAMING::HAS_MODEL_LOADED", "Hash", "BOOL"),
    ("STREAMING::SET_MODEL_AS_NO_LONGER_NEEDED", "Hash", "void"),
    ("VEHICLE::CREATE_VEHICLE", "Hash, float, float, float, float, BOOL, BOOL, BOOL", "Vehicle"),
    ("VEHICLE::SET_VEHICLE_ON_GROUND_PROPERLY", "Vehicle, float", "BOOL"),
    ("PED::SET_PED_INTO_VEHICLE", "Ped, Vehicle, int", "void"),
    ("MISC::SET_WEATHER_TYPE_NOW_PERSIST", "const char*", "void"),
    ("CLOCK::SET_CLOCK_TIME", "int, int, int", "void"),
    ("PLAYER::SET_PLAYER_WANTED_LEVEL", "Player, int, BOOL", "void"),
    ("PLAYER::SET_PLAYER_WANTED_LEVEL_NOW", "Player, BOOL", "void"),
    ("PLAYER::SET_MAX_WANTED_LEVEL", "int", "void"),
    ("PLAYER::CLEAR_PLAYER_WANTED_LEVEL", "Player", "void"),
    ("STATS::STAT_GET_INT", "Hash, int*, int", "BOOL"),
    ("STATS::STAT_SET_INT", "Hash, int, BOOL", "BOOL"),
    ("TEST::VEC_SCALE", "Vector3, float", "Vector3"),
    ("TEST::GET_POS_FIXUP", "Vector3*", "BOOL"),
    ("TEST::THREAD_ID", "0", "int"),
];

/// The target's identifier of a native.
pub fn id(name: &str) -> u64 {
    let j = scripthook_sdk::joaat(name) as u64;
    j << 32 | (j ^ 0x5348_4B21)
}

pub const HASH_XOR: u64 = 0x3C3C_3C3C_0000_0000;
pub const BUCKETS: u64 = 8;

/// Hex of a byte string, for patterns.
pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02X}")).collect::<Vec<_>>().join(" ")
}

pub const NATIVES_MAGIC: &[u8; 16] = b"SHK-TGT-NATIVES!";
pub const TICK_MAGIC: &[u8; 16] = b"SHK-TGT-TICKREF!";
pub const PLAYER_MAGIC: &[u8; 16] = b"SHK-TGT-PLAYER!!";

/// `natives.tsv` for the target (with or without its script tick).
pub fn tsv(tick: bool) -> String {
    let mut t = String::from("# natives of gta-vi-scripthook's synthetic test target (tests/target)\nidentifier\targs\treturns\tname\n");
    t.push_str(&format!(
        "@native_table\t{{ kind = \"buckets\", buckets = {BUCKETS}, bucket_index = \"modulo\", hash_xor = \"{HASH_XOR:#x}\", module = \"game\", section = \".data\", pattern = \"{}\", offset = 16 }}\n",
        hex(NATIVES_MAGIC)
    ));
    if tick {
        t.push_str(&format!("@script_tick\t{{ module = \"game\", section = \".data\", pattern = \"{}\", offset = 16, chain = [0] }}\n", hex(TICK_MAGIC)));
    }
    for (n, a, r) in NATIVES {
        t.push_str(&format!("{:#018x}\t{a}\t{r}\t{n}\n", id(n)));
    }
    t
}
