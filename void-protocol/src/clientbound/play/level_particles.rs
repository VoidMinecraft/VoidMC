use voidmc_codec::{Decode, DecodeError, Decoder, Encode, VarI32};
use voidmc_data::Version;

use crate::slot::DataComponentPatch;
use crate::types::BlockPosition;

const VERSION: Version = Version::V26_1_2;
const PARTICLE_TYPE: &str = "minecraft:particle_type";
const POSITION_SOURCE_TYPE: &str = "minecraft:position_source_type";

fn registry_id(registry: &str, name: &str) -> i32 {
    voidmc_data::protocol_registry_index(VERSION, registry, name)
        .unwrap_or_else(|| panic!("{name} is in the 26.1.2 {registry} registry"))
}

fn registry_name(registry: &str, id: i32) -> Option<&'static str> {
    voidmc_data::protocol_registry(VERSION, registry)?
        .iter()
        .find(|(_, candidate)| *candidate == id)
        .map(|(name, _)| *name)
}

fn unknown_id(id: i32) -> DecodeError {
    DecodeError::InvalidPacketId(u8::try_from(id).ok())
}

/// 0xAARRGGBB; `rgb` sets alpha to 0xFF (dust ignores alpha).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParticleColor(pub i32);

impl Encode for ParticleColor {
    fn encode(&self, buf: &mut Vec<u8>) {
        self.0.encode(buf);
    }
}

impl Decode for ParticleColor {
    fn decode_with(decoder: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        Ok(ParticleColor(decoder.decode()?))
    }
}

impl ParticleColor {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::argb(0xFF, r, g, b)
    }

    pub const fn argb(a: u8, r: u8, g: u8, b: u8) -> Self {
        ParticleColor(((a as i32) << 24) | ((r as i32) << 16) | ((g as i32) << 8) | b as i32)
    }
}

/// `ItemStackTemplate`: a non-empty stack (item, count, component patch).
#[derive(Debug, Clone, PartialEq)]
pub struct ItemStackTemplate {
    pub item_id: i32,
    pub count: i32,
    pub components: DataComponentPatch,
}

impl ItemStackTemplate {
    pub fn simple(item_id: i32, count: i32) -> Self {
        Self {
            item_id,
            count,
            components: DataComponentPatch::default(),
        }
    }
}

impl Encode for ItemStackTemplate {
    fn encode(&self, buf: &mut Vec<u8>) {
        VarI32(self.item_id).encode(buf);
        VarI32(self.count).encode(buf);
        self.components.encode(buf);
    }
}

impl Decode for ItemStackTemplate {
    fn decode_with(decoder: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            item_id: decoder.decode::<VarI32>()?.0,
            count: decoder.decode::<VarI32>()?.0,
            components: decoder.decode()?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PositionSource {
    Block(BlockPosition),
    Entity { entity_id: i32, y_offset: f32 },
}

impl PositionSource {
    pub fn name(&self) -> &'static str {
        match self {
            PositionSource::Block(_) => "minecraft:block",
            PositionSource::Entity { .. } => "minecraft:entity",
        }
    }
}

impl Encode for PositionSource {
    fn encode(&self, buf: &mut Vec<u8>) {
        VarI32(registry_id(POSITION_SOURCE_TYPE, self.name())).encode(buf);
        match self {
            PositionSource::Block(position) => position.encode(buf),
            PositionSource::Entity {
                entity_id,
                y_offset,
            } => {
                VarI32(*entity_id).encode(buf);
                y_offset.encode(buf);
            }
        }
    }
}

impl Decode for PositionSource {
    fn decode_with(decoder: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let type_id = decoder.decode::<VarI32>()?.0;
        Ok(
            match registry_name(POSITION_SOURCE_TYPE, type_id).ok_or(unknown_id(type_id))? {
                "minecraft:block" => PositionSource::Block(decoder.decode()?),
                "minecraft:entity" => PositionSource::Entity {
                    entity_id: decoder.decode::<VarI32>()?.0,
                    y_offset: decoder.decode()?,
                },
                _ => return Err(unknown_id(type_id)),
            },
        )
    }
}

macro_rules! particles {
    (
        data { $( $data:ident { $( $field:ident : $ty:ty ),* } => $data_name:literal, )* }
        simple { $( $simple:ident => $simple_name:literal, )* }
    ) => {
        /// Every `minecraft:particle_type` of 26.1.2; payload-carrying types
        /// carry their payload in the variant.
        #[derive(Debug, Clone, PartialEq)]
        pub enum Particle {
            $( $data { $( $field: $ty ),* }, )*
            $( $simple, )*
        }

        #[cfg(test)]
        impl Particle {
            fn fixtures() -> Vec<Particle> {
                vec![
                    $( Particle::$data { $( $field: tests::Fixture::fixture() ),* }, )*
                    $( Particle::$simple, )*
                ]
            }
        }

        impl Particle {
            pub const NAMES: &'static [&'static str] = &[$( $data_name, )* $( $simple_name, )*];

            pub fn name(&self) -> &'static str {
                match self {
                    $( Particle::$data { .. } => $data_name, )*
                    $( Particle::$simple => $simple_name, )*
                }
            }

            /// A payload-free particle by registry name (`minecraft:` prefix optional).
            pub fn simple(name: &str) -> Option<Particle> {
                match Self::qualify(name).as_str() {
                    $( $simple_name => Some(Particle::$simple), )*
                    _ => None,
                }
            }

            pub fn needs_data(name: &str) -> bool {
                matches!(Self::qualify(name).as_str(), $( $data_name )|*)
            }
        }
    };
}

particles! {
    data {
        Block { state: i32 } => "minecraft:block",
        BlockMarker { state: i32 } => "minecraft:block_marker",
        FallingDust { state: i32 } => "minecraft:falling_dust",
        DustPillar { state: i32 } => "minecraft:dust_pillar",
        BlockCrumble { state: i32 } => "minecraft:block_crumble",
        Dust { color: ParticleColor, scale: f32 } => "minecraft:dust",
        DustColorTransition { from: ParticleColor, to: ParticleColor, scale: f32 } => "minecraft:dust_color_transition",
        Effect { color: ParticleColor, power: f32 } => "minecraft:effect",
        InstantEffect { color: ParticleColor, power: f32 } => "minecraft:instant_effect",
        EntityEffect { color: ParticleColor } => "minecraft:entity_effect",
        TintedLeaves { color: ParticleColor } => "minecraft:tinted_leaves",
        Flash { color: ParticleColor } => "minecraft:flash",
        DragonBreath { power: f32 } => "minecraft:dragon_breath",
        Item { stack: ItemStackTemplate } => "minecraft:item",
        Vibration { source: PositionSource, arrival_in_ticks: i32 } => "minecraft:vibration",
        Trail { target: [f64; 3], color: ParticleColor, duration: i32 } => "minecraft:trail",
        SculkCharge { roll: f32 } => "minecraft:sculk_charge",
        Shriek { delay: i32 } => "minecraft:shriek",
    }
    simple {
        AngryVillager => "minecraft:angry_villager",
        Bubble => "minecraft:bubble",
        Cloud => "minecraft:cloud",
        CopperFireFlame => "minecraft:copper_fire_flame",
        Crit => "minecraft:crit",
        DamageIndicator => "minecraft:damage_indicator",
        DrippingLava => "minecraft:dripping_lava",
        FallingLava => "minecraft:falling_lava",
        LandingLava => "minecraft:landing_lava",
        DrippingWater => "minecraft:dripping_water",
        FallingWater => "minecraft:falling_water",
        ElderGuardian => "minecraft:elder_guardian",
        EnchantedHit => "minecraft:enchanted_hit",
        Enchant => "minecraft:enchant",
        EndRod => "minecraft:end_rod",
        ExplosionEmitter => "minecraft:explosion_emitter",
        Explosion => "minecraft:explosion",
        Gust => "minecraft:gust",
        SmallGust => "minecraft:small_gust",
        GustEmitterLarge => "minecraft:gust_emitter_large",
        GustEmitterSmall => "minecraft:gust_emitter_small",
        SonicBoom => "minecraft:sonic_boom",
        Firework => "minecraft:firework",
        Fishing => "minecraft:fishing",
        Flame => "minecraft:flame",
        Infested => "minecraft:infested",
        CherryLeaves => "minecraft:cherry_leaves",
        PaleOakLeaves => "minecraft:pale_oak_leaves",
        SculkSoul => "minecraft:sculk_soul",
        SculkChargePop => "minecraft:sculk_charge_pop",
        SoulFireFlame => "minecraft:soul_fire_flame",
        Soul => "minecraft:soul",
        HappyVillager => "minecraft:happy_villager",
        Composter => "minecraft:composter",
        Heart => "minecraft:heart",
        PauseMobGrowth => "minecraft:pause_mob_growth",
        ResetMobGrowth => "minecraft:reset_mob_growth",
        ItemSlime => "minecraft:item_slime",
        ItemCobweb => "minecraft:item_cobweb",
        ItemSnowball => "minecraft:item_snowball",
        LargeSmoke => "minecraft:large_smoke",
        Lava => "minecraft:lava",
        Mycelium => "minecraft:mycelium",
        Note => "minecraft:note",
        Poof => "minecraft:poof",
        Portal => "minecraft:portal",
        Rain => "minecraft:rain",
        Smoke => "minecraft:smoke",
        WhiteSmoke => "minecraft:white_smoke",
        Sneeze => "minecraft:sneeze",
        Spit => "minecraft:spit",
        SquidInk => "minecraft:squid_ink",
        SweepAttack => "minecraft:sweep_attack",
        TotemOfUndying => "minecraft:totem_of_undying",
        Underwater => "minecraft:underwater",
        Splash => "minecraft:splash",
        Witch => "minecraft:witch",
        BubblePop => "minecraft:bubble_pop",
        CurrentDown => "minecraft:current_down",
        BubbleColumnUp => "minecraft:bubble_column_up",
        Nautilus => "minecraft:nautilus",
        Dolphin => "minecraft:dolphin",
        CampfireCosySmoke => "minecraft:campfire_cosy_smoke",
        CampfireSignalSmoke => "minecraft:campfire_signal_smoke",
        DrippingHoney => "minecraft:dripping_honey",
        FallingHoney => "minecraft:falling_honey",
        LandingHoney => "minecraft:landing_honey",
        FallingNectar => "minecraft:falling_nectar",
        FallingSporeBlossom => "minecraft:falling_spore_blossom",
        Ash => "minecraft:ash",
        CrimsonSpore => "minecraft:crimson_spore",
        WarpedSpore => "minecraft:warped_spore",
        SporeBlossomAir => "minecraft:spore_blossom_air",
        DrippingObsidianTear => "minecraft:dripping_obsidian_tear",
        FallingObsidianTear => "minecraft:falling_obsidian_tear",
        LandingObsidianTear => "minecraft:landing_obsidian_tear",
        ReversePortal => "minecraft:reverse_portal",
        WhiteAsh => "minecraft:white_ash",
        SmallFlame => "minecraft:small_flame",
        Snowflake => "minecraft:snowflake",
        DrippingDripstoneLava => "minecraft:dripping_dripstone_lava",
        FallingDripstoneLava => "minecraft:falling_dripstone_lava",
        DrippingDripstoneWater => "minecraft:dripping_dripstone_water",
        FallingDripstoneWater => "minecraft:falling_dripstone_water",
        GlowSquidInk => "minecraft:glow_squid_ink",
        Glow => "minecraft:glow",
        WaxOn => "minecraft:wax_on",
        WaxOff => "minecraft:wax_off",
        ElectricSpark => "minecraft:electric_spark",
        Scrape => "minecraft:scrape",
        EggCrack => "minecraft:egg_crack",
        DustPlume => "minecraft:dust_plume",
        TrialSpawnerDetection => "minecraft:trial_spawner_detection",
        TrialSpawnerDetectionOminous => "minecraft:trial_spawner_detection_ominous",
        VaultConnection => "minecraft:vault_connection",
        OminousSpawning => "minecraft:ominous_spawning",
        RaidOmen => "minecraft:raid_omen",
        TrialOmen => "minecraft:trial_omen",
        Firefly => "minecraft:firefly",
    }
}

impl Particle {
    pub fn type_id(&self) -> i32 {
        registry_id(PARTICLE_TYPE, self.name())
    }

    fn qualify(name: &str) -> String {
        if name.contains(':') {
            name.to_string()
        } else {
            format!("minecraft:{name}")
        }
    }
}

impl Encode for Particle {
    fn encode(&self, buf: &mut Vec<u8>) {
        VarI32(self.type_id()).encode(buf);
        match self {
            Particle::Block { state }
            | Particle::BlockMarker { state }
            | Particle::FallingDust { state }
            | Particle::DustPillar { state }
            | Particle::BlockCrumble { state } => VarI32(*state).encode(buf),
            Particle::Dust { color, scale } => {
                color.encode(buf);
                scale.encode(buf);
            }
            Particle::DustColorTransition { from, to, scale } => {
                from.encode(buf);
                to.encode(buf);
                scale.encode(buf);
            }
            Particle::Effect { color, power } | Particle::InstantEffect { color, power } => {
                color.encode(buf);
                power.encode(buf);
            }
            Particle::EntityEffect { color }
            | Particle::TintedLeaves { color }
            | Particle::Flash { color } => color.encode(buf),
            Particle::DragonBreath { power } => power.encode(buf),
            Particle::Item { stack } => stack.encode(buf),
            Particle::Vibration {
                source,
                arrival_in_ticks,
            } => {
                source.encode(buf);
                VarI32(*arrival_in_ticks).encode(buf);
            }
            Particle::Trail {
                target,
                color,
                duration,
            } => {
                target[0].encode(buf);
                target[1].encode(buf);
                target[2].encode(buf);
                color.encode(buf);
                VarI32(*duration).encode(buf);
            }
            Particle::SculkCharge { roll } => roll.encode(buf),
            Particle::Shriek { delay } => VarI32(*delay).encode(buf),
            _ => {}
        }
    }
}

impl Decode for Particle {
    fn decode_with(decoder: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let type_id = decoder.decode::<VarI32>()?.0;
        let name = registry_name(PARTICLE_TYPE, type_id).ok_or(unknown_id(type_id))?;
        let state =
            |decoder: &mut Decoder<'_>| Ok::<i32, DecodeError>(decoder.decode::<VarI32>()?.0);
        Ok(match name {
            "minecraft:block" => Particle::Block {
                state: state(decoder)?,
            },
            "minecraft:block_marker" => Particle::BlockMarker {
                state: state(decoder)?,
            },
            "minecraft:falling_dust" => Particle::FallingDust {
                state: state(decoder)?,
            },
            "minecraft:dust_pillar" => Particle::DustPillar {
                state: state(decoder)?,
            },
            "minecraft:block_crumble" => Particle::BlockCrumble {
                state: state(decoder)?,
            },
            "minecraft:dust" => Particle::Dust {
                color: decoder.decode()?,
                scale: decoder.decode()?,
            },
            "minecraft:dust_color_transition" => Particle::DustColorTransition {
                from: decoder.decode()?,
                to: decoder.decode()?,
                scale: decoder.decode()?,
            },
            "minecraft:effect" => Particle::Effect {
                color: decoder.decode()?,
                power: decoder.decode()?,
            },
            "minecraft:instant_effect" => Particle::InstantEffect {
                color: decoder.decode()?,
                power: decoder.decode()?,
            },
            "minecraft:entity_effect" => Particle::EntityEffect {
                color: decoder.decode()?,
            },
            "minecraft:tinted_leaves" => Particle::TintedLeaves {
                color: decoder.decode()?,
            },
            "minecraft:flash" => Particle::Flash {
                color: decoder.decode()?,
            },
            "minecraft:dragon_breath" => Particle::DragonBreath {
                power: decoder.decode()?,
            },
            "minecraft:item" => Particle::Item {
                stack: decoder.decode()?,
            },
            "minecraft:vibration" => Particle::Vibration {
                source: decoder.decode()?,
                arrival_in_ticks: decoder.decode::<VarI32>()?.0,
            },
            "minecraft:trail" => Particle::Trail {
                target: [decoder.decode()?, decoder.decode()?, decoder.decode()?],
                color: decoder.decode()?,
                duration: decoder.decode::<VarI32>()?.0,
            },
            "minecraft:sculk_charge" => Particle::SculkCharge {
                roll: decoder.decode()?,
            },
            "minecraft:shriek" => Particle::Shriek {
                delay: decoder.decode::<VarI32>()?.0,
            },
            simple => Particle::simple(simple).ok_or(unknown_id(type_id))?,
        })
    }
}

/// Level Particles (0x2F). `count == 0` switches to directed mode: the offset
/// becomes a direction and `max_speed` its magnitude.
#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct LevelParticles {
    pub long_distance: bool,
    pub always_visible: bool,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub offset_x: f32,
    pub offset_y: f32,
    pub offset_z: f32,
    pub max_speed: f32,
    pub count: i32,
    pub particle: Particle,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clientbound::PlayPacket;

    pub(super) trait Fixture {
        fn fixture() -> Self;
    }

    impl Fixture for i32 {
        fn fixture() -> Self {
            300
        }
    }

    impl Fixture for f32 {
        fn fixture() -> Self {
            1.25
        }
    }

    impl Fixture for [f64; 3] {
        fn fixture() -> Self {
            [1.0, -2.0, 3.5]
        }
    }

    impl Fixture for ParticleColor {
        fn fixture() -> Self {
            ParticleColor::argb(9, 8, 7, 6)
        }
    }

    impl Fixture for ItemStackTemplate {
        fn fixture() -> Self {
            ItemStackTemplate::simple(1, 64)
        }
    }

    impl Fixture for PositionSource {
        fn fixture() -> Self {
            PositionSource::Entity {
                entity_id: 7,
                y_offset: 0.5,
            }
        }
    }

    fn encode(particle: Particle) -> Vec<u8> {
        let mut buf = Vec::new();
        particle.encode(&mut buf);
        buf
    }

    fn roundtrip(particle: &Particle) -> Particle {
        let buf = encode(particle.clone());
        let mut slice = buf.as_slice();
        let decoded = Particle::decode(&mut slice).expect("decode");
        assert!(slice.is_empty(), "trailing bytes for {}", particle.name());
        decoded
    }

    fn id(name: &str) -> u8 {
        voidmc_data::particle_type_id(VERSION, name).unwrap() as u8
    }

    #[test]
    fn every_name_resolves_and_matches_registry_size() {
        let registry = voidmc_data::protocol_registry(VERSION, PARTICLE_TYPE).unwrap();
        assert_eq!(Particle::NAMES.len(), registry.len());
        for name in Particle::NAMES {
            assert!(
                voidmc_data::particle_type_id(VERSION, name).is_some(),
                "{name} missing from registry"
            );
        }
        for (name, _) in registry {
            assert!(
                Particle::NAMES.contains(name),
                "{name} missing from Particle"
            );
        }
    }

    #[test]
    fn simple_lookup_accepts_bare_and_qualified_names() {
        assert_eq!(Particle::simple("flame"), Some(Particle::Flame));
        assert_eq!(Particle::simple("minecraft:flame"), Some(Particle::Flame));
        assert_eq!(Particle::simple("dust"), None);
        assert!(Particle::needs_data("dust"));
        assert!(!Particle::needs_data("flame"));
        assert!(!Particle::needs_data("not_a_particle"));
        assert_eq!(Particle::simple("not_a_particle"), None);
    }

    #[test]
    fn simple_particle_is_bare_registry_id() {
        assert_eq!(encode(Particle::Flame), vec![id("minecraft:flame")]);
        assert_eq!(encode(Particle::Firefly), vec![116]);
    }

    #[test]
    fn dust_is_int_color_then_float_scale() {
        let mut expected = vec![id("minecraft:dust")];
        expected.extend(0xFF_12_34_56_u32.to_be_bytes());
        expected.extend(1.5f32.to_be_bytes());
        assert_eq!(
            encode(Particle::Dust {
                color: ParticleColor::rgb(0x12, 0x34, 0x56),
                scale: 1.5,
            }),
            expected
        );
    }

    #[test]
    fn block_family_is_varint_state() {
        for particle in [
            Particle::Block { state: 300 },
            Particle::BlockMarker { state: 300 },
            Particle::FallingDust { state: 300 },
            Particle::DustPillar { state: 300 },
            Particle::BlockCrumble { state: 300 },
        ] {
            let bytes = encode(particle.clone());
            assert_eq!(bytes, vec![id(particle.name()), 0xAC, 0x02]);
        }
    }

    #[test]
    fn item_is_holder_count_and_empty_patch() {
        assert_eq!(
            encode(Particle::Item {
                stack: ItemStackTemplate::simple(5, 3),
            }),
            vec![id("minecraft:item"), 5, 3, 0, 0]
        );
    }

    #[test]
    fn entity_effect_is_argb_int() {
        let mut expected = vec![id("minecraft:entity_effect")];
        expected.extend(0x80_FF_00_10_u32.to_be_bytes());
        assert_eq!(
            encode(Particle::EntityEffect {
                color: ParticleColor::argb(0x80, 0xFF, 0x00, 0x10),
            }),
            expected
        );
    }

    #[test]
    fn vibration_and_trail_layouts() {
        let vibration = encode(Particle::Vibration {
            source: PositionSource::Entity {
                entity_id: 7,
                y_offset: 0.5,
            },
            arrival_in_ticks: 20,
        });
        let mut expected = vec![id("minecraft:vibration"), 1, 7];
        expected.extend(0.5f32.to_be_bytes());
        expected.push(20);
        assert_eq!(vibration, expected);

        let trail = encode(Particle::Trail {
            target: [1.0, 2.0, 3.0],
            color: ParticleColor(0x11),
            duration: 300,
        });
        let mut expected = vec![id("minecraft:trail")];
        for v in [1.0f64, 2.0, 3.0] {
            expected.extend(v.to_be_bytes());
        }
        expected.extend(0x11u32.to_be_bytes());
        expected.extend([0xAC, 0x02]);
        assert_eq!(trail, expected);
    }

    #[test]
    fn every_variant_roundtrips() {
        let fixtures = Particle::fixtures();
        assert_eq!(fixtures.len(), Particle::NAMES.len());
        for particle in fixtures {
            assert_eq!(roundtrip(&particle), particle);
        }
    }

    #[test]
    fn packet_layout_matches_paper() {
        let mut bytes = Vec::new();
        PlayPacket::LevelParticles(LevelParticles {
            long_distance: true,
            always_visible: false,
            x: 1.0,
            y: 2.0,
            z: 3.0,
            offset_x: 0.25,
            offset_y: 0.5,
            offset_z: 0.75,
            max_speed: 0.1,
            count: 8,
            particle: Particle::ElectricSpark,
        })
        .encode(&mut bytes);
        let mut expected = vec![0x2F, 1, 0];
        for v in [1.0f64, 2.0, 3.0] {
            expected.extend(v.to_be_bytes());
        }
        for v in [0.25f32, 0.5, 0.75, 0.1] {
            expected.extend(v.to_be_bytes());
        }
        expected.extend(8i32.to_be_bytes());
        expected.push(id("minecraft:electric_spark"));
        assert_eq!(bytes, expected);

        let decoded = PlayPacket::decode(&mut bytes.as_slice()).unwrap();
        assert!(matches!(
            decoded,
            PlayPacket::LevelParticles(LevelParticles {
                count: 8,
                particle: Particle::ElectricSpark,
                ..
            })
        ));
    }

    #[test]
    fn unknown_ids_are_rejected() {
        let bytes = [0xFF, 0x01];
        assert!(Particle::decode(&mut bytes.as_slice()).is_err());
        let bytes = [0x02];
        assert!(PositionSource::decode(&mut bytes.as_slice()).is_err());
    }

    #[test]
    fn position_source_ids_come_from_the_registry() {
        let mut block = Vec::new();
        PositionSource::Block(BlockPosition { x: 0, y: 0, z: 0 }).encode(&mut block);
        assert_eq!(block[0], 0);
        let mut entity = Vec::new();
        PositionSource::Entity {
            entity_id: 1,
            y_offset: 0.0,
        }
        .encode(&mut entity);
        assert_eq!(entity[0], 1);
        assert_eq!(
            PositionSource::decode(&mut block.as_slice()).unwrap(),
            PositionSource::Block(BlockPosition { x: 0, y: 0, z: 0 })
        );
    }
}
