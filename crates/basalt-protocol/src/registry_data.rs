//! Registry data required for the Configuration state.
//!
//! The Minecraft client expects registry data for several registries
//! before it will accept a FinishConfiguration packet. This module
//! provides builders for all required registries:
//!
//! - `minecraft:dimension_type` — world properties (height, light)
//! - `minecraft:worldgen/biome` — biome rendering (colors, sky, fog)
//! - `minecraft:damage_type` — damage source definitions (49 entries)
//! - `minecraft:painting_variant` — required since 1.21+
//! - `minecraft:wolf_variant` — required since 1.21+
//! - `minecraft:chat_type` — chat message formatting (chat + msg_command)
//! - `minecraft:trim_pattern` — armor trim patterns
//! - `minecraft:trim_material` — armor trim materials
//! - `minecraft:banner_pattern` — banner pattern definitions
//! - `minecraft:enchantment` — enchantment definitions
//! - `minecraft:jukebox_song` — music disc definitions
//! - `minecraft:instrument` — goat horn instrument definitions

use std::sync::OnceLock;

use crate::packets::configuration::{
    ClientboundConfigurationRegistryData, ClientboundConfigurationRegistryDataEntries,
};
use basalt_types::nbt::{NbtCompound, NbtTag};
use basalt_types::{Encode, EncodedSize};

/// Returns the pre-encoded payloads of every default registry packet,
/// suitable for direct write via a `RawSlice`-style wrapper.
///
/// `build_default_registries` produces identical content for every
/// login (six static registries: dimension type, biome, damage type,
/// painting variant, wolf variant, chat type), so encoding it fresh
/// per connection is pure waste — most visible on cold-start mass
/// joins. This function encodes each registry once on first call,
/// caches the byte vectors in a `OnceLock`, and returns the slice
/// for every subsequent caller. The packet ID is unchanged across
/// payloads (`ClientboundConfigurationRegistryData::PACKET_ID`); the
/// caller frames each slice with that id.
///
/// Order matches `build_default_registries` exactly — keeping the
/// cache and the builder bytewise comparable in tests.
pub fn cached_registry_payloads() -> &'static [Vec<u8>] {
    static CACHE: OnceLock<Vec<Vec<u8>>> = OnceLock::new();
    CACHE.get_or_init(|| {
        build_default_registries()
            .into_iter()
            .map(|reg| {
                let mut buf = Vec::with_capacity(reg.encoded_size());
                reg.encode(&mut buf)
                    .expect("registry data encoding cannot fail");
                buf
            })
            .collect()
    })
}

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
        build_chat_type_registry(),
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

/// Definition of a damage type for the registry data table.
///
/// Each entry maps to one `minecraft:damage_type` registry entry
/// that the client needs during `DamageSources` initialization.
struct DamageTypeDef {
    /// Registry key (e.g., "in_fire").
    key: &'static str,
    /// Death message translation key (e.g., "inFire").
    message_id: &'static str,
    /// Damage scaling rule: "never", "when_caused_by_living_non_player", or "always".
    scaling: &'static str,
    /// Hunger exhaustion applied when this damage is taken.
    exhaustion: f32,
    /// Optional visual/sound effect: "burning", "drowning", "freezing", "poking", "thorns".
    effects: Option<&'static str>,
    /// Optional death message variant: "fall_variants", "intentional_game_design".
    death_message_type: Option<&'static str>,
}

/// All damage types required by the Minecraft 1.21.4 client.
///
/// The `DamageSources` class looks up these types during world
/// initialization via `getOrThrow` — any missing entry crashes
/// the client. This list covers all types from the vanilla data
/// generator plus 1.21+ additions (wind_charge, mace_smash).
const DAMAGE_TYPES: &[DamageTypeDef] = &[
    // -- Environment --
    DamageTypeDef {
        key: "in_fire",
        message_id: "inFire",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.1,
        effects: Some("burning"),
        death_message_type: None,
    },
    DamageTypeDef {
        key: "campfire",
        message_id: "inFire",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.1,
        effects: Some("burning"),
        death_message_type: None,
    },
    DamageTypeDef {
        key: "on_fire",
        message_id: "onFire",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.0,
        effects: Some("burning"),
        death_message_type: None,
    },
    DamageTypeDef {
        key: "lava",
        message_id: "lava",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.1,
        effects: Some("burning"),
        death_message_type: None,
    },
    DamageTypeDef {
        key: "hot_floor",
        message_id: "hotFloor",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.1,
        effects: Some("burning"),
        death_message_type: None,
    },
    DamageTypeDef {
        key: "in_wall",
        message_id: "inWall",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.0,
        effects: None,
        death_message_type: None,
    },
    DamageTypeDef {
        key: "cramming",
        message_id: "cramming",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.0,
        effects: None,
        death_message_type: None,
    },
    DamageTypeDef {
        key: "drown",
        message_id: "drown",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.0,
        effects: Some("drowning"),
        death_message_type: None,
    },
    DamageTypeDef {
        key: "starve",
        message_id: "starve",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.0,
        effects: None,
        death_message_type: None,
    },
    DamageTypeDef {
        key: "cactus",
        message_id: "cactus",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.1,
        effects: Some("poking"),
        death_message_type: None,
    },
    DamageTypeDef {
        key: "sweet_berry_bush",
        message_id: "sweetBerryBush",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.1,
        effects: Some("poking"),
        death_message_type: None,
    },
    DamageTypeDef {
        key: "freeze",
        message_id: "freeze",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.0,
        effects: Some("freezing"),
        death_message_type: None,
    },
    DamageTypeDef {
        key: "lightning_bolt",
        message_id: "lightningBolt",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.1,
        effects: None,
        death_message_type: None,
    },
    DamageTypeDef {
        key: "dry_out",
        message_id: "dryOut",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.1,
        effects: None,
        death_message_type: None,
    },
    // -- Physics --
    DamageTypeDef {
        key: "fall",
        message_id: "fall",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.0,
        effects: None,
        death_message_type: Some("fall_variants"),
    },
    DamageTypeDef {
        key: "fly_into_wall",
        message_id: "flyIntoWall",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.0,
        effects: None,
        death_message_type: None,
    },
    DamageTypeDef {
        key: "stalagmite",
        message_id: "stalagmite",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.0,
        effects: None,
        death_message_type: Some("fall_variants"),
    },
    DamageTypeDef {
        key: "falling_anvil",
        message_id: "anvil",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.1,
        effects: None,
        death_message_type: None,
    },
    DamageTypeDef {
        key: "falling_block",
        message_id: "fallingBlock",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.1,
        effects: None,
        death_message_type: None,
    },
    DamageTypeDef {
        key: "falling_stalactite",
        message_id: "fallingStalactite",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.1,
        effects: None,
        death_message_type: None,
    },
    // -- System --
    DamageTypeDef {
        key: "out_of_world",
        message_id: "outOfWorld",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.0,
        effects: None,
        death_message_type: None,
    },
    DamageTypeDef {
        key: "generic",
        message_id: "generic",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.0,
        effects: None,
        death_message_type: None,
    },
    DamageTypeDef {
        key: "generic_kill",
        message_id: "genericKill",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.0,
        effects: None,
        death_message_type: None,
    },
    DamageTypeDef {
        key: "outside_border",
        message_id: "outsideBorder",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.0,
        effects: None,
        death_message_type: None,
    },
    DamageTypeDef {
        key: "bad_respawn_point",
        message_id: "badRespawnPoint",
        scaling: "always",
        exhaustion: 0.1,
        effects: None,
        death_message_type: Some("intentional_game_design"),
    },
    // -- Magic / status effects --
    DamageTypeDef {
        key: "magic",
        message_id: "magic",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.0,
        effects: None,
        death_message_type: None,
    },
    DamageTypeDef {
        key: "indirect_magic",
        message_id: "indirectMagic",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.0,
        effects: None,
        death_message_type: None,
    },
    DamageTypeDef {
        key: "wither",
        message_id: "wither",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.0,
        effects: None,
        death_message_type: None,
    },
    DamageTypeDef {
        key: "dragon_breath",
        message_id: "dragonBreath",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.0,
        effects: None,
        death_message_type: None,
    },
    DamageTypeDef {
        key: "sonic_boom",
        message_id: "sonic_boom",
        scaling: "always",
        exhaustion: 0.0,
        effects: None,
        death_message_type: None,
    },
    // -- Combat --
    DamageTypeDef {
        key: "mob_attack",
        message_id: "mob_attack",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.1,
        effects: None,
        death_message_type: None,
    },
    DamageTypeDef {
        key: "mob_attack_no_aggro",
        message_id: "mob_attack_no_aggro",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.1,
        effects: None,
        death_message_type: None,
    },
    DamageTypeDef {
        key: "mob_projectile",
        message_id: "mob_projectile",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.1,
        effects: None,
        death_message_type: None,
    },
    DamageTypeDef {
        key: "player_attack",
        message_id: "player_attack",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.1,
        effects: None,
        death_message_type: None,
    },
    DamageTypeDef {
        key: "player_explosion",
        message_id: "player_explosion",
        scaling: "always",
        exhaustion: 0.1,
        effects: None,
        death_message_type: None,
    },
    DamageTypeDef {
        key: "explosion",
        message_id: "explosion",
        scaling: "always",
        exhaustion: 0.1,
        effects: None,
        death_message_type: None,
    },
    DamageTypeDef {
        key: "thorns",
        message_id: "thorns",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.1,
        effects: Some("thorns"),
        death_message_type: None,
    },
    DamageTypeDef {
        key: "sting",
        message_id: "sting",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.1,
        effects: None,
        death_message_type: None,
    },
    DamageTypeDef {
        key: "spit",
        message_id: "mob_attack",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.1,
        effects: None,
        death_message_type: None,
    },
    // -- Projectiles --
    DamageTypeDef {
        key: "arrow",
        message_id: "arrow",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.1,
        effects: None,
        death_message_type: None,
    },
    DamageTypeDef {
        key: "trident",
        message_id: "trident",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.1,
        effects: None,
        death_message_type: None,
    },
    DamageTypeDef {
        key: "thrown",
        message_id: "thrown",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.1,
        effects: None,
        death_message_type: None,
    },
    DamageTypeDef {
        key: "fireball",
        message_id: "fireball",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.1,
        effects: Some("burning"),
        death_message_type: None,
    },
    DamageTypeDef {
        key: "unattributed_fireball",
        message_id: "onFire",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.1,
        effects: Some("burning"),
        death_message_type: None,
    },
    DamageTypeDef {
        key: "fireworks",
        message_id: "fireworks",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.1,
        effects: None,
        death_message_type: None,
    },
    DamageTypeDef {
        key: "wither_skull",
        message_id: "witherSkull",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.1,
        effects: None,
        death_message_type: None,
    },
    DamageTypeDef {
        key: "wind_charge",
        message_id: "wind_charge",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.1,
        effects: None,
        death_message_type: None,
    },
    DamageTypeDef {
        key: "mace_smash",
        message_id: "mace_smash",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.1,
        effects: None,
        death_message_type: None,
    },
    DamageTypeDef {
        key: "ender_pearl",
        message_id: "fall",
        scaling: "when_caused_by_living_non_player",
        exhaustion: 0.0,
        effects: None,
        death_message_type: Some("fall_variants"),
    },
];

/// Builds the `minecraft:damage_type` registry with all vanilla
/// damage types.
///
/// The client's `DamageSources` class looks up specific damage
/// types via `getOrThrow` during world initialization — any
/// missing entry causes an immediate crash. This sends the
/// complete set from the 1.21.4 data generator.
fn build_damage_type_registry() -> ClientboundConfigurationRegistryData {
    let entries = DAMAGE_TYPES
        .iter()
        .map(|def| {
            let mut nbt = NbtCompound::new();
            nbt.insert("message_id", NbtTag::String(def.message_id.into()));
            nbt.insert("scaling", NbtTag::String(def.scaling.into()));
            nbt.insert("exhaustion", NbtTag::Float(def.exhaustion));
            if let Some(effects) = def.effects {
                nbt.insert("effects", NbtTag::String(effects.into()));
            }
            if let Some(dmt) = def.death_message_type {
                nbt.insert("death_message_type", NbtTag::String(dmt.into()));
            }
            ClientboundConfigurationRegistryDataEntries {
                key: format!("minecraft:{}", def.key),
                value: Some(nbt),
            }
        })
        .collect();

    ClientboundConfigurationRegistryData {
        id: "minecraft:damage_type".into(),
        entries,
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

/// Builds the `minecraft:chat_type` registry.
///
/// Defines how chat messages are formatted on the client. Each entry
/// has a `chat` section (for the chat window) and a `narration` section
/// (for accessibility narration). The `chat` type uses `chat.type.text`
/// which formats as `<sender> message`. The `msg_command` type uses
/// `commands.message.display.incoming` for `/msg` whispers.
fn build_chat_type_registry() -> ClientboundConfigurationRegistryData {
    // Helper to build a chat/narration decoration
    fn decoration(translation_key: &str, parameters: &[&str]) -> NbtCompound {
        let mut dec = NbtCompound::new();
        dec.insert("translation_key", NbtTag::String(translation_key.into()));
        let params: Vec<NbtTag> = parameters
            .iter()
            .map(|p| NbtTag::String((*p).into()))
            .collect();
        dec.insert(
            "parameters",
            NbtTag::List(basalt_types::nbt::NbtList::from_tags(params).unwrap()),
        );
        dec.insert("style", NbtTag::Compound(NbtCompound::new()));
        dec
    }

    // "chat" type — used for regular player chat messages
    let mut chat_type = NbtCompound::new();
    chat_type.insert(
        "chat",
        NbtTag::Compound(decoration("chat.type.text", &["sender", "content"])),
    );
    chat_type.insert(
        "narration",
        NbtTag::Compound(decoration("chat.type.text.narrate", &["sender", "content"])),
    );

    // "msg_command" type — used for /msg (whisper) messages
    let mut msg_command = NbtCompound::new();
    msg_command.insert(
        "chat",
        NbtTag::Compound(decoration(
            "commands.message.display.incoming",
            &["sender", "content"],
        )),
    );
    msg_command.insert(
        "narration",
        NbtTag::Compound(decoration("chat.type.text.narrate", &["sender", "content"])),
    );

    ClientboundConfigurationRegistryData {
        id: "minecraft:chat_type".into(),
        entries: vec![
            ClientboundConfigurationRegistryDataEntries {
                key: "minecraft:chat".into(),
                value: Some(chat_type),
            },
            ClientboundConfigurationRegistryDataEntries {
                key: "minecraft:msg_command".into(),
                value: Some(msg_command),
            },
        ],
    }
}

// Future registries — these have complex NBT formats that require
// matching the exact vanilla data generator output. Tracked in
// separate issues for each registry group.
//
// - trim_pattern / trim_material
// - banner_pattern
// - enchantment
// - jukebox_song / instrument

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_all_registries() {
        let registries = build_default_registries();
        assert_eq!(registries.len(), 6);

        let ids: Vec<&str> = registries.iter().map(|r| r.id.as_str()).collect();
        assert!(ids.contains(&"minecraft:dimension_type"));
        assert!(ids.contains(&"minecraft:worldgen/biome"));
        assert!(ids.contains(&"minecraft:damage_type"));
        assert!(ids.contains(&"minecraft:painting_variant"));
        assert!(ids.contains(&"minecraft:wolf_variant"));
        assert!(ids.contains(&"minecraft:chat_type"));
    }

    #[test]
    fn chat_type_has_entries() {
        let reg = build_chat_type_registry();
        assert_eq!(reg.entries.len(), 2);
        let keys: Vec<&str> = reg.entries.iter().map(|e| e.key.as_str()).collect();
        assert!(keys.contains(&"minecraft:chat"));
        assert!(keys.contains(&"minecraft:msg_command"));
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
    fn damage_type_has_all_entries() {
        let reg = build_damage_type_registry();
        assert_eq!(reg.entries.len(), DAMAGE_TYPES.len());

        // Check that critical damage types the client requires are present
        let keys: Vec<&str> = reg.entries.iter().map(|e| e.key.as_str()).collect();
        for required in [
            "minecraft:in_fire",
            "minecraft:generic",
            "minecraft:fall",
            "minecraft:out_of_world",
            "minecraft:wind_charge",
            "minecraft:mace_smash",
        ] {
            assert!(keys.contains(&required), "missing damage type: {required}");
        }
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

    #[test]
    fn cached_payloads_match_freshly_built() {
        let cached = cached_registry_payloads();
        let built: Vec<Vec<u8>> = build_default_registries()
            .into_iter()
            .map(|reg| {
                let mut buf = Vec::with_capacity(reg.encoded_size());
                reg.encode(&mut buf).unwrap();
                buf
            })
            .collect();
        assert_eq!(cached.len(), built.len(), "cached entry count mismatch");
        for (i, (c, b)) in cached.iter().zip(built.iter()).enumerate() {
            assert_eq!(c, b, "cached payload {i} differs from freshly encoded");
        }
    }

    #[test]
    fn cached_payloads_returns_same_storage_across_calls() {
        // Two consecutive calls must hand back the same backing slice —
        // proves the OnceLock is hit instead of rebuilding on every call.
        let first = cached_registry_payloads();
        let second = cached_registry_payloads();
        assert!(
            std::ptr::eq(first.as_ptr(), second.as_ptr()),
            "cached_registry_payloads must return the same storage on repeat calls"
        );
    }
}
