// Every `#[codec(packet_id = ..)]` and manual `VarI32(..)` prefix in this crate
// is checked against Mojang's packets.json. New variants go in `SPECS` under
// their Mojang name; new files holding packet enums go in `SPECS` too.

use voidmc_data::{Version, packet_id, packets};

const VERSION: Version = Version::V26_1_2;

struct EnumSpec {
    file: &'static str,
    source: &'static str,
    state: &'static str,
    direction: &'static str,
    names: &'static [(&'static str, &'static str)],
}

const SPECS: &[EnumSpec] = &[
    EnumSpec {
        file: "src/serverbound/handshake.rs",
        source: include_str!("../src/serverbound/handshake.rs"),
        state: "handshake",
        direction: "serverbound",
        names: &[("Handshake", "minecraft:intention")],
    },
    EnumSpec {
        file: "src/clientbound/status.rs",
        source: include_str!("../src/clientbound/status.rs"),
        state: "status",
        direction: "clientbound",
        names: &[
            ("StatusResponse", "minecraft:status_response"),
            ("PingResponse", "minecraft:pong_response"),
        ],
    },
    EnumSpec {
        file: "src/serverbound/status.rs",
        source: include_str!("../src/serverbound/status.rs"),
        state: "status",
        direction: "serverbound",
        names: &[
            ("StatusRequest", "minecraft:status_request"),
            ("PingRequest", "minecraft:ping_request"),
        ],
    },
    EnumSpec {
        file: "src/clientbound/login.rs",
        source: include_str!("../src/clientbound/login.rs"),
        state: "login",
        direction: "clientbound",
        names: &[("LoginSuccess", "minecraft:login_finished")],
    },
    EnumSpec {
        file: "src/serverbound/login.rs",
        source: include_str!("../src/serverbound/login.rs"),
        state: "login",
        direction: "serverbound",
        names: &[
            ("LoginStart", "minecraft:hello"),
            ("LoginAcknowledged", "minecraft:login_acknowledged"),
        ],
    },
    EnumSpec {
        file: "src/clientbound/configuration.rs",
        source: include_str!("../src/clientbound/configuration.rs"),
        state: "configuration",
        direction: "clientbound",
        names: &[
            ("FinishConfiguration", "minecraft:finish_configuration"),
            ("RegistryData", "minecraft:registry_data"),
            ("KnownPacks", "minecraft:select_known_packs"),
            ("UpdateTags", "minecraft:update_tags"),
        ],
    },
    EnumSpec {
        file: "src/serverbound/configuration.rs",
        source: include_str!("../src/serverbound/configuration.rs"),
        state: "configuration",
        direction: "serverbound",
        names: &[
            ("ClientInformation", "minecraft:client_information"),
            ("PluginMessage", "minecraft:custom_payload"),
            (
                "FinishConfigurationAcknowledged",
                "minecraft:finish_configuration",
            ),
            ("KnownPacks", "minecraft:select_known_packs"),
        ],
    },
    EnumSpec {
        file: "src/clientbound/play.rs",
        source: include_str!("../src/clientbound/play.rs"),
        state: "play",
        direction: "clientbound",
        names: &[
            ("SpawnEntity", "minecraft:add_entity"),
            ("BlockChangedAck", "minecraft:block_changed_ack"),
            ("BlockEntityData", "minecraft:block_entity_data"),
            ("BlockUpdate", "minecraft:block_update"),
            ("BossEvent", "minecraft:boss_event"),
            ("ChunksBiomes", "minecraft:chunks_biomes"),
            ("CloseContainer", "minecraft:container_close"),
            ("SetContainerContent", "minecraft:container_set_content"),
            ("SetContainerSlot", "minecraft:container_set_slot"),
            ("SetCooldown", "minecraft:cooldown"),
            ("Disconnect", "minecraft:disconnect"),
            ("UnloadChunk", "minecraft:forget_level_chunk"),
            ("GameEvent", "minecraft:game_event"),
            ("KeepAlive", "minecraft:keep_alive"),
            ("LevelParticles", "minecraft:level_particles"),
            ("Login", "minecraft:login"),
            ("UpdateEntityPosition", "minecraft:move_entity_pos"),
            (
                "UpdateEntityPositionAndRotation",
                "minecraft:move_entity_pos_rot",
            ),
            ("UpdateEntityRotation", "minecraft:move_entity_rot"),
            ("OpenScreen", "minecraft:open_screen"),
            ("Ping", "minecraft:ping"),
            ("PlayerAbilities", "minecraft:player_abilities"),
            ("SynchronizePlayerPosition", "minecraft:player_position"),
            ("SetHeadRotation", "minecraft:rotate_head"),
            ("EntitySoundEffect", "minecraft:sound_entity"),
            ("SoundEffect", "minecraft:sound"),
            ("StopSound", "minecraft:stop_sound"),
            ("SetEntityMotion", "minecraft:set_entity_motion"),
            ("SetCenterChunk", "minecraft:set_chunk_cache_center"),
            ("SetCursorItem", "minecraft:set_cursor_item"),
            ("SetEntityData", "minecraft:set_entity_data"),
            ("SetHeldSlot", "minecraft:set_held_slot"),
            ("ResetScore", "minecraft:reset_score"),
            ("SetDisplayObjective", "minecraft:set_display_objective"),
            ("SetObjective", "minecraft:set_objective"),
            ("SetPlayerTeam", "minecraft:set_player_team"),
            ("SetScore", "minecraft:set_score"),
            ("SystemChat", "minecraft:system_chat"),
            ("TeleportEntity", "minecraft:teleport_entity"),
            ("PlayerInfoUpdate", "minecraft:player_info_update"),
            ("PlayerInfoRemove", "minecraft:player_info_remove"),
            ("RemoveEntities", "minecraft:remove_entities"),
            ("ChunkDataAndLight", "minecraft:level_chunk_with_light"),
            ("Commands", "minecraft:commands"),
            (
                "CommandSuggestionsResponse",
                "minecraft:command_suggestions",
            ),
            ("SetPassengers", "minecraft:set_passengers"),
        ],
    },
    EnumSpec {
        file: "src/serverbound/play.rs",
        source: include_str!("../src/serverbound/play.rs"),
        state: "play",
        direction: "serverbound",
        names: &[
            ("ConfirmTeleportation", "minecraft:accept_teleportation"),
            ("ChatCommand", "minecraft:chat_command"),
            ("SignedChatCommand", "minecraft:chat_command_signed"),
            ("ChatMessage", "minecraft:chat"),
            ("TickEnd", "minecraft:client_tick_end"),
            ("ClientInformation", "minecraft:client_information"),
            ("CommandSuggestionsRequest", "minecraft:command_suggestion"),
            ("ClickContainer", "minecraft:container_click"),
            ("CloseContainer", "minecraft:container_close"),
            ("Interact", "minecraft:interact"),
            ("KeepAlive", "minecraft:keep_alive"),
            ("SetPlayerPos", "minecraft:move_player_pos"),
            ("SetPlayerPosAndRot", "minecraft:move_player_pos_rot"),
            ("SetPlayerRotation", "minecraft:move_player_rot"),
            ("PlayerAbilities", "minecraft:player_abilities"),
            ("PlayerAction", "minecraft:player_action"),
            ("PlayerCommand", "minecraft:player_command"),
            ("PlayerInput", "minecraft:player_input"),
            ("PlayerLoaded", "minecraft:player_loaded"),
            ("Pong", "minecraft:pong"),
            ("SetHeldItem", "minecraft:set_carried_item"),
            ("SetCreativeModeSlot", "minecraft:set_creative_mode_slot"),
            ("SwingArm", "minecraft:swing"),
            ("UseItemOn", "minecraft:use_item_on"),
            ("UseItem", "minecraft:use_item"),
        ],
    },
];

#[derive(Debug)]
struct Literal {
    variant: String,
    id: i32,
    tagged: bool,
}

fn parse_int(text: &str) -> Option<i32> {
    let text = text.trim();
    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        i32::from_str_radix(hex, 16).ok()
    } else {
        text.parse().ok()
    }
}

fn extract_literals(source: &str) -> Vec<Literal> {
    let lines: Vec<&str> = source.lines().collect();
    let mut out = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();

        if let Some(rest) = trimmed.strip_prefix("#[codec(packet_id = ") {
            let id_text = rest
                .split(')')
                .next()
                .unwrap_or_else(|| panic!("malformed packet_id attribute: {trimmed}"));
            let id = parse_int(id_text)
                .unwrap_or_else(|| panic!("packet_id is not an integer literal: {trimmed}"));
            let variant_line = lines[i + 1..]
                .iter()
                .map(|l| l.trim())
                .find(|l| !l.is_empty() && !l.starts_with("//") && !l.starts_with("#["))
                .unwrap_or_else(|| panic!("packet_id attribute without a variant: {trimmed}"));
            let variant = variant_line
                .split(['(', ',', ' '])
                .next()
                .unwrap_or_default()
                .to_string();
            out.push(Literal {
                variant,
                id,
                tagged: true,
            });
            continue;
        }

        if trimmed.starts_with("Manual") && trimmed.contains("Packet::") && trimmed.contains("=>") {
            let pos = trimmed.find("Packet::").unwrap() + "Packet::".len();
            let variant = trimmed[pos..]
                .split('(')
                .next()
                .unwrap_or_default()
                .to_string();
            let next = lines.get(i + 1).map(|l| l.trim()).unwrap_or_default();
            let id_text = if trimmed.ends_with("=> {") {
                next.split("VarI32(")
                    .nth(1)
                    .and_then(|s| s.split(')').next())
            } else {
                None
            };
            let id_text = id_text.unwrap_or_else(|| {
                panic!(
                    "manual encoder arm for {variant} must be `=> {{` followed by a \
                     `VarI32(<id>)` line so its id can be checked: {trimmed}"
                )
            });
            let id = parse_int(id_text)
                .unwrap_or_else(|| panic!("manual packet id is not a literal: {next}"));
            out.push(Literal {
                variant,
                id,
                tagged: false,
            });
        }
    }
    out
}

#[test]
fn every_hand_written_packet_id_matches_mojang_report() {
    let mut failures = Vec::new();
    let mut checked = 0;

    for spec in SPECS {
        let literals = extract_literals(spec.source);
        assert!(
            !literals.is_empty(),
            "{}: no packet ids found — has the file layout changed?",
            spec.file
        );

        for (variant, name) in spec.names {
            assert!(
                packet_id(VERSION, spec.state, spec.direction, name).is_some(),
                "{}: SPECS maps {variant} to {name:?}, which is not a {}/{} packet in packets.json",
                spec.file,
                spec.state,
                spec.direction
            );
        }

        for lit in &literals {
            let Some((_, name)) = spec.names.iter().find(|(v, _)| *v == lit.variant) else {
                failures.push(format!(
                    "{}: variant {} has no `names` entry in SPECS of tests/packet_ids.rs",
                    spec.file, lit.variant
                ));
                continue;
            };
            let expected =
                packet_id(VERSION, spec.state, spec.direction, name).expect("checked above");
            if lit.id != expected {
                failures.push(format!(
                    "{}: {} is written as {:#04x} but {}/{} {name} is {:#04x} in packets.json",
                    spec.file, lit.variant, lit.id, spec.state, spec.direction, expected
                ));
            }
            // The derive writes the id as a single byte; a VarInt only fits
            // one byte below 0x80.
            if lit.tagged && lit.id >= 0x80 {
                failures.push(format!(
                    "{}: {} = {:#04x} needs a two-byte VarInt but the tagged-enum derive writes one byte",
                    spec.file, lit.variant, lit.id
                ));
            }
            checked += 1;
        }
    }

    assert!(
        failures.is_empty(),
        "packet id drift detected:\n  {}",
        failures.join("\n  ")
    );
    assert!(
        checked >= 60,
        "only {checked} packet ids checked — extraction broke?"
    );
}

#[test]
fn every_packet_enum_file_is_in_specs() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_rs(&src, &mut files);
    let mut missing = Vec::new();
    for path in files {
        let text = std::fs::read_to_string(&path).unwrap();
        if !(text.contains("packet_id =") || text.contains("impl Encode for Manual")) {
            continue;
        }
        let rel = path
            .strip_prefix(env!("CARGO_MANIFEST_DIR"))
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if !SPECS.iter().any(|s| s.file == rel) {
            missing.push(rel);
        }
    }
    assert!(
        missing.is_empty(),
        "files with packet ids but no SPECS entry in tests/packet_ids.rs:\n  {}",
        missing.join("\n  ")
    );
}

fn collect_rs(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn packet_tables_exist_for_every_state_and_direction() {
    for spec in SPECS {
        let table = packets(VERSION, spec.state, spec.direction)
            .unwrap_or_else(|| panic!("no {}/{} table", spec.state, spec.direction));
        assert!(!table.is_empty());
    }
}
