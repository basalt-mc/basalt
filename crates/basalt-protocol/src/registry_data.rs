//! Minimum registry data required for the Configuration state.
//!
//! The Minecraft client expects registry data for several registries
//! before it will accept a FinishConfiguration packet. This module
//! provides builders for the minimum required registries:
//!
//! - `minecraft:dimension_type` — world properties (height, light)
//! - `minecraft:worldgen/biome` — biome rendering (colors, sky, fog)
//! - `minecraft:damage_type` — damage source definitions
//! - `minecraft:painting_variant` — required since 1.21+
//! - `minecraft:wolf_variant` — required since 1.21+
//!
//! These are the minimum registries that prevent the client from
//! crashing or refusing to enter Play state.

use crate::packets::configuration::{
    ClientboundConfigurationRegistryData, ClientboundConfigurationRegistryDataEntries,
};
use basalt_types::nbt::{NbtCompound, NbtTag};

/// Builds all required registry data packets for the Configuration state.
///
/// Returns a list of `ClientboundConfigurationRegistryData` packets,
/// one per registry. These should all be sent before `FinishConfiguration`.
pub fn build_default_registries() -> Vec<ClientboundConfigurationRegistryData> {
    vec![
        build_dimension_type_registry(),
        build_biome_registry(),
        build_damage_type_registry(),
        build_painting_variant_registry(),
        build_wolf_variant_registry(),
    ]
}

/// Builds the `minecraft:dimension_type` registry with a single
/// overworld dimension type.
///
/// The dimension type defines world properties like height range,
/// ambient light, natural spawning, and coordinate scale.
fn build_dimension_type_registry() -> ClientboundConfigurationRegistryData {
    let mut overworld = NbtCompound::new();
    overworld.insert("has_skylight", NbtTag::Byte(1));
    overworld.insert("has_ceiling", NbtTag::Byte(0));
    overworld.insert("ultrawarm", NbtTag::Byte(0));
    overworld.insert("natural", NbtTag::Byte(1));
    overworld.insert("coordinate_scale", NbtTag::Double(1.0));
    overworld.insert("bed_works", NbtTag::Byte(1));
    overworld.insert("respawn_anchor_works", NbtTag::Byte(0));
    overworld.insert("min_y", NbtTag::Int(-64));
    overworld.insert("height", NbtTag::Int(384));
    overworld.insert("logical_height", NbtTag::Int(384));
    overworld.insert(
        "infiniburn",
        NbtTag::String("#minecraft:infiniburn_overworld".into()),
    );
    overworld.insert("effects", NbtTag::String("minecraft:overworld".into()));
    overworld.insert("ambient_light", NbtTag::Float(0.0));
    overworld.insert("piglin_safe", NbtTag::Byte(0));
    overworld.insert("has_raids", NbtTag::Byte(1));
    overworld.insert("monster_spawn_light_level", NbtTag::Int(0));
    overworld.insert("monster_spawn_block_light_limit", NbtTag::Int(0));

    ClientboundConfigurationRegistryData {
        id: "minecraft:dimension_type".into(),
        entries: vec![ClientboundConfigurationRegistryDataEntries {
            key: "minecraft:overworld".into(),
            value: Some(overworld),
        }],
    }
}

/// Builds the `minecraft:worldgen/biome` registry with a single
/// plains biome.
///
/// The biome defines rendering properties: sky color, fog color,
/// water color, grass/foliage modifiers, and weather.
fn build_biome_registry() -> ClientboundConfigurationRegistryData {
    let mut effects = NbtCompound::new();
    effects.insert("sky_color", NbtTag::Int(7907327));
    effects.insert("water_fog_color", NbtTag::Int(329011));
    effects.insert("fog_color", NbtTag::Int(12638463));
    effects.insert("water_color", NbtTag::Int(4159204));

    let mut plains = NbtCompound::new();
    plains.insert("has_precipitation", NbtTag::Byte(1));
    plains.insert("temperature", NbtTag::Float(0.8));
    plains.insert("downfall", NbtTag::Float(0.4));
    plains.insert("effects", NbtTag::Compound(effects));

    ClientboundConfigurationRegistryData {
        id: "minecraft:worldgen/biome".into(),
        entries: vec![ClientboundConfigurationRegistryDataEntries {
            key: "minecraft:plains".into(),
            value: Some(plains),
        }],
    }
}

/// Builds the `minecraft:damage_type` registry with a single
/// generic damage type.
///
/// The client requires at least one damage type to initialize
/// its damage system, even if no damage occurs.
fn build_damage_type_registry() -> ClientboundConfigurationRegistryData {
    let mut generic = NbtCompound::new();
    generic.insert("message_id", NbtTag::String("generic".into()));
    generic.insert("scaling", NbtTag::String("never".into()));
    generic.insert("exhaustion", NbtTag::Float(0.0));

    ClientboundConfigurationRegistryData {
        id: "minecraft:damage_type".into(),
        entries: vec![ClientboundConfigurationRegistryDataEntries {
            key: "minecraft:generic".into(),
            value: Some(generic),
        }],
    }
}

/// Builds the `minecraft:painting_variant` registry with a single
/// painting variant.
///
/// Required since 1.21+ — the client crashes without it.
fn build_painting_variant_registry() -> ClientboundConfigurationRegistryData {
    let mut kebab = NbtCompound::new();
    kebab.insert("asset_id", NbtTag::String("minecraft:kebab".into()));
    kebab.insert("width", NbtTag::Int(1));
    kebab.insert("height", NbtTag::Int(1));

    ClientboundConfigurationRegistryData {
        id: "minecraft:painting_variant".into(),
        entries: vec![ClientboundConfigurationRegistryDataEntries {
            key: "minecraft:kebab".into(),
            value: Some(kebab),
        }],
    }
}

/// Builds the `minecraft:wolf_variant` registry with a single
/// wolf variant.
///
/// Required since 1.21+ — the client crashes without it.
fn build_wolf_variant_registry() -> ClientboundConfigurationRegistryData {
    let mut pale = NbtCompound::new();
    pale.insert(
        "wild_texture",
        NbtTag::String("minecraft:entity/wolf/wolf".into()),
    );
    pale.insert(
        "tame_texture",
        NbtTag::String("minecraft:entity/wolf/wolf_tame".into()),
    );
    pale.insert(
        "angry_texture",
        NbtTag::String("minecraft:entity/wolf/wolf_angry".into()),
    );
    pale.insert("biomes", NbtTag::String("minecraft:plains".into()));

    ClientboundConfigurationRegistryData {
        id: "minecraft:wolf_variant".into(),
        entries: vec![ClientboundConfigurationRegistryDataEntries {
            key: "minecraft:pale".into(),
            value: Some(pale),
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use basalt_types::{Encode, EncodedSize};

    #[test]
    fn build_all_registries() {
        let registries = build_default_registries();
        assert_eq!(registries.len(), 5);

        assert_eq!(registries[0].id, "minecraft:dimension_type");
        assert_eq!(registries[1].id, "minecraft:worldgen/biome");
        assert_eq!(registries[2].id, "minecraft:damage_type");
        assert_eq!(registries[3].id, "minecraft:painting_variant");
        assert_eq!(registries[4].id, "minecraft:wolf_variant");
    }

    #[test]
    fn dimension_type_has_entries() {
        let reg = build_dimension_type_registry();
        assert_eq!(reg.entries.len(), 1);
        assert_eq!(reg.entries[0].key, "minecraft:overworld");
        assert!(reg.entries[0].value.is_some());
    }

    #[test]
    fn biome_has_effects() {
        let reg = build_biome_registry();
        let value = reg.entries[0].value.as_ref().unwrap();
        assert!(value.get("effects").is_some());
    }

    #[test]
    fn registries_encode() {
        let registries = build_default_registries();
        for reg in &registries {
            let mut buf = Vec::with_capacity(reg.encoded_size());
            reg.encode(&mut buf).unwrap();
            assert!(
                !buf.is_empty(),
                "registry {} should encode to non-empty bytes",
                reg.id
            );
        }
    }
}
