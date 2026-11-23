//! The generated natives.rs (from natives/examples/test-title.tsv) compiles against the SDK and
//! has the expected shape.

#[allow(dead_code)]
mod natives {
    include!(concat!(env!("OUT_DIR"), "/c/natives.rs"));
}

#[test]
fn generated_rust_bindings_compile() {
    // typed signatures (not called: natives need a host and a script fiber)
    let _: fn() -> natives::Result<natives::Ped> = natives::player::player_ped_id;
    let _: fn(natives::Entity, bool) -> natives::Result<natives::Vec3> = natives::entity::get_entity_coords;
    let _: fn(natives::Hash, &mut i32, i32) -> natives::Result<bool> = natives::stats::stat_get_int;
    let _: fn(&str) -> natives::Result<()> = natives::misc::set_weather_type_now_persist;
    let _: fn(u64, u64, u64, u64, u64, u64, u64) -> natives::Result<()> = natives::entity::set_entity_coords_no_offset;
}
