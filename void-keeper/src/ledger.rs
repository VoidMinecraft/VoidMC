//! What the server has added to a client, kept by id so that it can be taken back.
//!
//! A server restart forgets what the client still shows. Whatever a server sends as a whole
//! replaces itself (commands, tags, recipes) and needs nothing here. What adds up is noted by
//! id, and [`Ledger::teardown`] undoes it like a destructor, before the new server speaks.
//!
//! Every clientbound play packet of the protocol falls in one of three cases, listed in
//! [`Ledger::note`]: it leaves no state, it leaves state this ledger can undo, or it leaves
//! state it cannot, and then the client must go through a reconfiguration. An unknown packet
//! or a packet that cannot be read counts as the last case: a ledger that lost track of the
//! client never claims the client is clean.

use std::collections::HashSet;

use bytes::Bytes;
use ussr_nbt::owned::{Nbt, Tag};
use uuid::Uuid;
use voidmc_codec::{DecodeError, DecodeLimits, Decoder, Encode, VarI32};
use voidmc_data::v26_1_2::packets::play::clientbound as cb;
use voidmc_data::v26_1_2::{EntityAttribute, EntityKind};
use voidmc_protocol::clientbound::{
    AttributeSnapshot, BossEvent, BossEventAction, ClearDialog, ClearTitles, CloseContainer,
    EntityMetadataEntry, EntityMetadataValue, GameEvent, GameEventType, InitializeBorder, Login,
    ManualPlayPacket, ObjectiveAction, PlayPacket, PlayerAbilities, PlayerInfoActions,
    PlayerInfoEntry, PlayerInfoRemove, PlayerInfoUpdate, RemoveEntities, RemoveMobEffect, Respawn,
    SetCamera, SetContainerContent, SetCooldown, SetCursorItem, SetEntityData, SetHeldSlot,
    SetObjective, SetPassengers, SetPlayerTeam, SetTabListHeaderFooter, TeamAction, UnloadChunk,
    UpdateAttributes,
};
use voidmc_protocol::slot::Slot;

use crate::wire::{frame, split, tagged};

const NO_LOGIN: &str = "the client never received a login";
const PLAYER_MENU_SLOTS: usize = 46;
const FRESH_BORDER_DIAMETER: f64 = 59_999_968.0;
const FRESH_PORTAL_BOUNDARY: i32 = 29_999_984;
const FRESH_FLYING_SPEED: f32 = 0.05;
const FRESH_WALKING_SPEED: f32 = 0.1;

/// What the `login` of a server decides for good: a client cannot change it without a new level.
#[derive(Debug, Clone, PartialEq)]
struct Shape {
    hardcore: bool,
    dimensions: Vec<String>,
    reduced_debug_info: bool,
    respawn_screen: bool,
    limited_crafting: bool,
    dimension_type: i32,
    dimension: String,
    hashed_seed: i64,
    debug: bool,
    flat: bool,
    sea_level: i32,
    secure_chat: bool,
}

impl Shape {
    fn of(login: &Login) -> Self {
        Self {
            hardcore: login.is_hardcore,
            dimensions: login.dimension_names.clone(),
            reduced_debug_info: login.reduced_debug_info,
            respawn_screen: login.enable_respawn_screen,
            limited_crafting: login.do_limited_crafting,
            dimension_type: login.dimension_type,
            dimension: login.dimension_name.clone(),
            hashed_seed: login.hashed_seed,
            debug: login.is_debug,
            flat: login.is_flat,
            sea_level: login.sea_level,
            secure_chat: login.enforces_secure_chat,
        }
    }
}

#[derive(Debug, Clone)]
struct Standing {
    player: i32,
    shape: Shape,
    view_distance: i32,
    simulation_distance: i32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Abilities {
    flags: u8,
    flying_speed: f32,
    walking_speed: f32,
}

impl Abilities {
    fn of_game_mode(mode: u8) -> Self {
        let flags = match mode {
            1 => {
                PlayerAbilities::INVULNERABLE
                    | PlayerAbilities::ALLOW_FLYING
                    | PlayerAbilities::INSTANT_BUILD
            }
            3 => {
                PlayerAbilities::INVULNERABLE
                    | PlayerAbilities::ALLOW_FLYING
                    | PlayerAbilities::FLYING
            }
            _ => 0,
        };
        Self {
            flags,
            flying_speed: FRESH_FLYING_SPEED,
            walking_speed: FRESH_WALKING_SPEED,
        }
    }
}

/// How much a ledger holds, for the logs and the events.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tally {
    pub entities: usize,
    pub chunks: usize,
    pub boss_bars: usize,
    pub objectives: usize,
    pub teams: usize,
    pub tab_entries: usize,
    pub effects: usize,
    pub bytes: usize,
}

#[derive(Debug, Default)]
pub struct Ledger {
    standing: Option<Standing>,
    game_mode: u8,
    abilities: Option<Abilities>,

    entities: HashSet<i32>,
    chunks: HashSet<(i32, i32)>,
    stale_chunks: HashSet<(i32, i32)>,
    boss_bars: HashSet<Uuid>,
    objectives: HashSet<String>,
    teams: HashSet<String>,
    tab_entries: HashSet<Uuid>,
    effects: HashSet<i32>,
    attributes: HashSet<i32>,
    cooldowns: HashSet<String>,

    camera: Option<i32>,
    vehicle: Option<i32>,
    carries_passengers: bool,
    menu: Option<i32>,
    dialog: bool,
    bundle_open: bool,

    player_data: bool,
    tab_header: bool,
    border: bool,
    rain: bool,
    tick_rate: bool,
    chat_completions: bool,
    advancements: bool,
    recipes: bool,
    health: bool,
    experience: bool,
    inventory: bool,
    cursor: bool,

    tainted: Option<String>,
}

impl Ledger {
    /// Takes note of a frame on its way to the client. Never fails: a frame it cannot read
    /// still reaches the client, and taints the ledger.
    pub fn observe(&mut self, frame: &[u8]) {
        let Some((id, body)) = split(frame) else {
            self.taint("a packet without an id");
            return;
        };
        let mut decoder = Decoder::new(body, DecodeLimits::default());
        if let Err(error) = self.note(id, &mut decoder) {
            tracing::warn!(id, %error, "clientbound packet not understood");
            self.taint(format!("packet {id:#x} could not be read: {error}"));
        }
    }

    fn taint(&mut self, reason: impl Into<String>) {
        self.tainted.get_or_insert_with(|| reason.into());
    }

    pub fn tainted(&self) -> Option<&str> {
        self.tainted.as_deref()
    }

    /// Why no `login` can be taken over seamlessly, whatever it says.
    pub fn unfit(&self) -> Option<&str> {
        match self.standing {
            None => Some(NO_LOGIN),
            Some(_) => self.tainted(),
        }
    }

    fn is_player(&self, entity: i32) -> bool {
        self.standing
            .as_ref()
            .is_some_and(|standing| standing.player == entity)
    }

    fn note(&mut self, id: i32, packet: &mut Decoder<'_>) -> Result<(), DecodeError> {
        match id {
            cb::LOGIN => self.note_login(&packet.decode()?),
            cb::RESPAWN => self.note_respawn(packet.decode()?),
            cb::BUNDLE_DELIMITER => self.bundle_open = !self.bundle_open,

            cb::ADD_ENTITY => {
                self.entities.insert(packet.decode::<VarI32>()?.0);
            }
            cb::REMOVE_ENTITIES => {
                for _ in 0..packet.decode::<VarI32>()?.0 {
                    let entity = packet.decode::<VarI32>()?.0;
                    self.entities.remove(&entity);
                    if self.camera == Some(entity) {
                        self.camera = None;
                    }
                    if self.vehicle == Some(entity) {
                        self.vehicle = None;
                    }
                }
            }
            cb::LEVEL_CHUNK_WITH_LIGHT => {
                let chunk = (packet.decode::<i32>()?, packet.decode::<i32>()?);
                self.chunks.insert(chunk);
                self.stale_chunks.remove(&chunk);
            }
            cb::FORGET_LEVEL_CHUNK => {
                let forgotten = packet.decode::<UnloadChunk>()?;
                let chunk = (forgotten.chunk_x, forgotten.chunk_z);
                self.chunks.remove(&chunk);
                self.stale_chunks.remove(&chunk);
            }
            cb::BOSS_EVENT => {
                let id = packet.decode::<Uuid>()?;
                match packet.decode::<VarI32>()?.0 {
                    0 => {
                        self.boss_bars.insert(id);
                    }
                    1 => {
                        self.boss_bars.remove(&id);
                    }
                    _ => {}
                }
            }
            cb::SET_OBJECTIVE => {
                let name = packet.decode::<String>()?;
                match packet.decode::<u8>()? {
                    0 => {
                        self.objectives.insert(name);
                    }
                    1 => {
                        self.objectives.remove(&name);
                    }
                    _ => {}
                }
            }
            cb::SET_PLAYER_TEAM => {
                let name = packet.decode::<String>()?;
                match packet.decode::<u8>()? {
                    0 => {
                        self.teams.insert(name);
                    }
                    1 => {
                        self.teams.remove(&name);
                    }
                    _ => {}
                }
            }
            cb::SET_CHUNK_CACHE_RADIUS => {
                let radius = packet.decode::<VarI32>()?.0;
                if let Some(standing) = &mut self.standing {
                    standing.view_distance = radius;
                }
            }
            cb::SET_SIMULATION_DISTANCE => {
                let distance = packet.decode::<VarI32>()?.0;
                if let Some(standing) = &mut self.standing {
                    standing.simulation_distance = distance;
                }
            }
            cb::PLAYER_INFO_UPDATE => self.note_tab_entries(packet)?,
            cb::PLAYER_INFO_REMOVE => {
                for _ in 0..packet.decode::<VarI32>()?.0 {
                    self.tab_entries.remove(&packet.decode::<Uuid>()?);
                }
            }
            cb::UPDATE_MOB_EFFECT => {
                if self.is_player(packet.decode::<VarI32>()?.0) {
                    self.effects.insert(packet.decode::<VarI32>()?.0);
                }
            }
            cb::REMOVE_MOB_EFFECT => {
                if self.is_player(packet.decode::<VarI32>()?.0) {
                    self.effects.remove(&packet.decode::<VarI32>()?.0);
                }
            }
            cb::UPDATE_ATTRIBUTES => {
                if self.is_player(packet.decode::<VarI32>()?.0) {
                    for _ in 0..packet.decode::<VarI32>()?.0 {
                        let attribute = packet.decode::<AttributeSnapshot>()?;
                        self.attributes.insert(attribute.attribute_id);
                    }
                }
            }
            cb::SET_ENTITY_DATA => {
                self.player_data |= self.is_player(packet.decode::<VarI32>()?.0);
            }
            cb::COOLDOWN => {
                let cooldown = packet.decode::<SetCooldown>()?;
                if cooldown.duration == 0 {
                    self.cooldowns.remove(&cooldown.cooldown_group);
                } else {
                    self.cooldowns.insert(cooldown.cooldown_group);
                }
            }
            cb::SET_CAMERA => {
                let camera = packet.decode::<SetCamera>()?.camera_id;
                self.camera = (!self.is_player(camera)).then_some(camera);
            }
            cb::SET_PASSENGERS => self.note_passengers(packet)?,
            cb::OPEN_SCREEN | cb::MOUNT_SCREEN_OPEN => {
                self.menu = Some(packet.decode::<VarI32>()?.0);
            }
            cb::CONTAINER_CLOSE => self.menu = None,
            cb::SHOW_DIALOG => self.dialog = true,
            cb::CLEAR_DIALOG => self.dialog = false,
            cb::GAME_EVENT => self.note_game_event(packet)?,
            cb::PLAYER_ABILITIES => {
                let abilities = packet.decode::<PlayerAbilities>()?;
                self.abilities = Some(Abilities {
                    flags: abilities.flags,
                    flying_speed: abilities.flying_speed,
                    walking_speed: abilities.walking_speed,
                });
            }

            cb::TAB_LIST => self.tab_header = true,
            cb::INITIALIZE_BORDER
            | cb::SET_BORDER_CENTER
            | cb::SET_BORDER_LERP_SIZE
            | cb::SET_BORDER_SIZE
            | cb::SET_BORDER_WARNING_DELAY
            | cb::SET_BORDER_WARNING_DISTANCE => self.border = true,
            cb::TICKING_STATE | cb::TICKING_STEP => self.tick_rate = true,
            cb::CUSTOM_CHAT_COMPLETIONS => self.chat_completions = true,
            cb::UPDATE_ADVANCEMENTS => self.advancements = true,
            cb::RECIPE_BOOK_ADD => self.recipes = true,
            cb::SET_HEALTH => self.health = true,
            cb::SET_EXPERIENCE => self.experience = true,
            cb::CONTAINER_SET_CONTENT | cb::CONTAINER_SET_SLOT | cb::SET_PLAYER_INVENTORY => {
                self.inventory = true;
            }
            cb::SET_CURSOR_ITEM => self.cursor = true,

            cb::BLOCK_DESTRUCTION => self.taint("a block is cracked by a breaker id"),
            cb::WAYPOINT => self.taint("the locator bar holds a waypoint"),
            cb::OPEN_BOOK => self.taint("a book is open"),
            cb::OPEN_SIGN_EDITOR => self.taint("a sign is being edited"),
            cb::PLAYER_COMBAT_KILL => self.taint("the death screen is open"),

            cb::ANIMATE
            | cb::AWARD_STATS
            | cb::BLOCK_CHANGED_ACK
            | cb::BLOCK_ENTITY_DATA
            | cb::BLOCK_EVENT
            | cb::BLOCK_UPDATE
            | cb::CHANGE_DIFFICULTY
            | cb::CHUNK_BATCH_FINISHED
            | cb::CHUNK_BATCH_START
            | cb::CHUNKS_BIOMES
            | cb::CLEAR_TITLES
            | cb::COMMAND_SUGGESTIONS
            | cb::COMMANDS
            | cb::CONTAINER_SET_DATA
            | cb::COOKIE_REQUEST
            | cb::CUSTOM_PAYLOAD
            | cb::CUSTOM_REPORT_DETAILS
            | cb::DAMAGE_EVENT
            | cb::DEBUG_BLOCK_VALUE
            | cb::DEBUG_CHUNK_VALUE
            | cb::DEBUG_ENTITY_VALUE
            | cb::DEBUG_EVENT
            | cb::DEBUG_SAMPLE
            | cb::DELETE_CHAT
            | cb::DISCONNECT
            | cb::DISGUISED_CHAT
            | cb::ENTITY_EVENT
            | cb::ENTITY_POSITION_SYNC
            | cb::EXPLODE
            | cb::GAME_RULE_VALUES
            | cb::GAME_TEST_HIGHLIGHT_POS
            | cb::HURT_ANIMATION
            | cb::KEEP_ALIVE
            | cb::LEVEL_EVENT
            | cb::LEVEL_PARTICLES
            | cb::LIGHT_UPDATE
            | cb::LOW_DISK_SPACE_WARNING
            | cb::MAP_ITEM_DATA
            | cb::MERCHANT_OFFERS
            | cb::MOVE_ENTITY_POS
            | cb::MOVE_ENTITY_POS_ROT
            | cb::MOVE_ENTITY_ROT
            | cb::MOVE_MINECART_ALONG_TRACK
            | cb::MOVE_VEHICLE
            | cb::PING
            | cb::PLACE_GHOST_RECIPE
            | cb::PLAYER_CHAT
            | cb::PLAYER_COMBAT_END
            | cb::PLAYER_COMBAT_ENTER
            | cb::PLAYER_LOOK_AT
            | cb::PLAYER_POSITION
            | cb::PLAYER_ROTATION
            | cb::PONG_RESPONSE
            | cb::PROJECTILE_POWER
            | cb::RECIPE_BOOK_REMOVE
            | cb::RECIPE_BOOK_SETTINGS
            | cb::RESET_SCORE
            | cb::RESOURCE_PACK_POP
            | cb::RESOURCE_PACK_PUSH
            | cb::ROTATE_HEAD
            | cb::SECTION_BLOCKS_UPDATE
            | cb::SELECT_ADVANCEMENTS_TAB
            | cb::SERVER_DATA
            | cb::SERVER_LINKS
            | cb::SET_ACTION_BAR_TEXT
            | cb::SET_CHUNK_CACHE_CENTER
            | cb::SET_DEFAULT_SPAWN_POSITION
            | cb::SET_DISPLAY_OBJECTIVE
            | cb::SET_ENTITY_LINK
            | cb::SET_ENTITY_MOTION
            | cb::SET_EQUIPMENT
            | cb::SET_HELD_SLOT
            | cb::SET_SCORE
            | cb::SET_SUBTITLE_TEXT
            | cb::SET_TIME
            | cb::SET_TITLE_TEXT
            | cb::SET_TITLES_ANIMATION
            | cb::SOUND
            | cb::SOUND_ENTITY
            | cb::START_CONFIGURATION
            | cb::STOP_SOUND
            | cb::STORE_COOKIE
            | cb::SYSTEM_CHAT
            | cb::TAG_QUERY
            | cb::TAKE_ITEM_ENTITY
            | cb::TELEPORT_ENTITY
            | cb::TEST_INSTANCE_BLOCK_STATUS
            | cb::TRANSFER
            | cb::UPDATE_RECIPES
            | cb::UPDATE_TAGS => {}

            unknown => self.taint(format!("packet {unknown:#x} is not in the ledger")),
        }
        Ok(())
    }

    fn note_login(&mut self, login: &Login) {
        self.standing = Some(Standing {
            player: login.entity_id,
            shape: Shape::of(login),
            view_distance: login.view_distance,
            simulation_distance: login.simulation_distance,
        });
        self.game_mode = login.game_mode;
        self.abilities = None;
    }

    fn note_respawn(&mut self, respawn: Respawn) {
        self.game_mode = respawn.game_mode;
        let Some(standing) = &mut self.standing else {
            return;
        };
        if standing.shape.dimension != respawn.dimension_name {
            self.entities.clear();
            self.chunks.clear();
            self.camera = None;
            self.vehicle = None;
        }
        standing.shape.dimension = respawn.dimension_name;
        standing.shape.dimension_type = respawn.dimension_type;
        standing.shape.hashed_seed = respawn.hashed_seed;
        standing.shape.debug = respawn.is_debug;
        standing.shape.flat = respawn.is_flat;
        standing.shape.sea_level = respawn.sea_level;
    }

    fn note_tab_entries(&mut self, packet: &mut Decoder<'_>) -> Result<(), DecodeError> {
        let actions = packet.decode::<u8>()?;
        const ADD_PLAYER: u8 = 0x01;
        const INITIALIZE_CHAT: u8 = 0x02;
        const UPDATE_GAME_MODE: u8 = 0x04;
        const UPDATE_LISTED: u8 = 0x08;
        const UPDATE_LATENCY: u8 = 0x10;
        const UPDATE_DISPLAY_NAME: u8 = 0x20;
        const UPDATE_LIST_ORDER: u8 = 0x40;
        const UPDATE_HAT: u8 = 0x80;
        if actions & ADD_PLAYER == 0 {
            return Ok(());
        }
        for _ in 0..packet.decode::<VarI32>()?.0 {
            self.tab_entries.insert(packet.decode::<Uuid>()?);
            packet.decode::<String>()?;
            for _ in 0..packet.decode::<VarI32>()?.0 {
                packet.decode::<String>()?;
                packet.decode::<String>()?;
                packet.decode::<Option<String>>()?;
            }
            if actions & INITIALIZE_CHAT != 0 && packet.decode::<bool>()? {
                packet.decode::<Uuid>()?;
                packet.decode::<i64>()?;
                skip_bytes(packet)?;
                skip_bytes(packet)?;
            }
            if actions & UPDATE_GAME_MODE != 0 {
                packet.decode::<VarI32>()?;
            }
            if actions & UPDATE_LISTED != 0 {
                packet.decode::<bool>()?;
            }
            if actions & UPDATE_LATENCY != 0 {
                packet.decode::<VarI32>()?;
            }
            if actions & UPDATE_DISPLAY_NAME != 0 {
                packet.decode::<Option<Nbt>>()?;
            }
            if actions & UPDATE_LIST_ORDER != 0 {
                packet.decode::<VarI32>()?;
            }
            if actions & UPDATE_HAT != 0 {
                packet.decode::<bool>()?;
            }
        }
        Ok(())
    }

    fn note_passengers(&mut self, packet: &mut Decoder<'_>) -> Result<(), DecodeError> {
        let vehicle = packet.decode::<VarI32>()?.0;
        let count = packet.decode::<VarI32>()?.0;
        let mut rides = false;
        for _ in 0..count {
            rides |= self.is_player(packet.decode::<VarI32>()?.0);
        }
        if self.is_player(vehicle) {
            self.carries_passengers = count > 0;
        } else if rides {
            self.vehicle = Some(vehicle);
        } else if self.vehicle == Some(vehicle) {
            self.vehicle = None;
        }
        Ok(())
    }

    fn note_game_event(&mut self, packet: &mut Decoder<'_>) -> Result<(), DecodeError> {
        let event = packet.decode::<GameEvent>()?;
        match event.event {
            GameEventType::ChangeGameMode => {
                self.game_mode = event.value as u8;
                self.abilities = None;
            }
            GameEventType::BeginRaining
            | GameEventType::RainLevelChange
            | GameEventType::ThunderLevelChange => self.rain = true,
            GameEventType::WinGame => self.taint("the end credits are rolling"),
            GameEventType::EnableRespawnScreen => self.taint("the respawn screen was switched"),
            GameEventType::LimitedCrafting => self.taint("limited crafting was switched"),
            _ => {}
        }
        Ok(())
    }

    /// Whether a seamless return is possible onto this `login`, and why not otherwise.
    pub fn fits(&self, login: &Login) -> Result<(), String> {
        let Some(standing) = &self.standing else {
            return Err(NO_LOGIN.to_string());
        };
        if let Some(reason) = &self.tainted {
            return Err(reason.clone());
        }
        if standing.player != login.entity_id {
            return Err(format!(
                "player id changed from {} to {}; start the server with \
                 VOIDMC_STABLE_PLAYER_IDS=1 to stay in the world",
                standing.player, login.entity_id
            ));
        }
        if standing.shape != Shape::of(login) {
            return Err(format!(
                "the level changed: {:?} became {:?}",
                standing.shape,
                Shape::of(login)
            ));
        }
        Ok(())
    }

    /// A bundle cut short would swallow every packet that follows it.
    pub fn close_bundle(&mut self) -> Option<Bytes> {
        self.bundle_open.then(|| {
            self.bundle_open = false;
            frame(cb::BUNDLE_DELIMITER, |_| {})
        })
    }

    /// Everything that undoes this ledger on the client and puts it where `login` would have.
    /// Afterwards the ledger holds what the client holds: its own tab entry, and its old chunks,
    /// which stay on screen until [`take_stale_chunks`](Self::take_stale_chunks).
    pub fn teardown(&mut self, login: &Login, profile: Uuid) -> Vec<Bytes> {
        let mut frames = Vec::new();
        let player = login.entity_id;
        let play = |packet: PlayPacket| tagged(&packet);
        let manual = |packet: ManualPlayPacket| tagged(&packet);

        if std::mem::take(&mut self.dialog) {
            frames.push(play(PlayPacket::ClearDialog(ClearDialog {})));
        }
        if let Some(container_id) = self.menu.take() {
            frames.push(play(PlayPacket::CloseContainer(CloseContainer {
                container_id,
            })));
        }
        if self.camera.take().is_some() {
            frames.push(play(PlayPacket::SetCamera(SetCamera { camera_id: player })));
        }
        if let Some(entity_id) = self.vehicle.take() {
            frames.push(manual(ManualPlayPacket::SetPassengers(SetPassengers {
                entity_id,
                passengers: Vec::new(),
            })));
        }
        if std::mem::take(&mut self.carries_passengers) {
            frames.push(manual(ManualPlayPacket::SetPassengers(SetPassengers {
                entity_id: player,
                passengers: Vec::new(),
            })));
        }
        let mut entity_ids: Vec<i32> = self.entities.drain().collect();
        entity_ids.retain(|entity| *entity != player);
        if !entity_ids.is_empty() {
            frames.push(manual(ManualPlayPacket::RemoveEntities(RemoveEntities {
                entity_ids,
            })));
        }
        for id in self.boss_bars.drain() {
            frames.push(play(PlayPacket::BossEvent(BossEvent {
                id,
                action: BossEventAction::Remove,
            })));
        }
        for name in self.objectives.drain() {
            frames.push(play(PlayPacket::SetObjective(SetObjective {
                name,
                action: ObjectiveAction::Remove,
            })));
        }
        for name in self.teams.drain() {
            frames.push(play(PlayPacket::SetPlayerTeam(SetPlayerTeam {
                name,
                action: TeamAction::Remove,
            })));
        }
        // A local player keeps the entry it first read for as long as it lives, and takes its
        // game mode from it: a new entry would never reach it.
        let own_entry = self.tab_entries.remove(&profile);
        if !self.tab_entries.is_empty() {
            frames.push(manual(ManualPlayPacket::PlayerInfoRemove(
                PlayerInfoRemove {
                    uuids: self.tab_entries.drain().collect(),
                },
            )));
        }
        if own_entry {
            self.tab_entries.insert(profile);
            frames.push(manual(ManualPlayPacket::PlayerInfoUpdate(
                PlayerInfoUpdate::single(
                    PlayerInfoActions::UPDATE_GAME_MODE,
                    PlayerInfoEntry {
                        uuid: profile,
                        game_mode: i32::from(login.game_mode),
                        ..Default::default()
                    },
                ),
            )));
        }
        frames.push(play(PlayPacket::ClearTitles(ClearTitles {
            reset_times: true,
        })));
        if std::mem::take(&mut self.tab_header) {
            frames.push(play(PlayPacket::SetTabListHeaderFooter(
                SetTabListHeaderFooter {
                    header: text(""),
                    footer: text(""),
                },
            )));
        }
        for effect_id in self.effects.drain() {
            frames.push(play(PlayPacket::RemoveMobEffect(RemoveMobEffect {
                entity_id: player,
                effect_id,
            })));
        }
        if !self.attributes.is_empty() {
            let attributes = self
                .attributes
                .drain()
                .filter_map(|attribute_id| {
                    let attribute = EntityAttribute::from_id(attribute_id)?;
                    Some(AttributeSnapshot {
                        attribute_id,
                        base: EntityKind::Player.default_attribute(attribute),
                        modifiers: Vec::new(),
                    })
                })
                .collect();
            frames.push(play(PlayPacket::UpdateAttributes(UpdateAttributes {
                entity_id: player,
                attributes,
            })));
        }
        if std::mem::take(&mut self.player_data) {
            frames.push(play(PlayPacket::SetEntityData(fresh_player_data(player))));
        }
        for cooldown_group in self.cooldowns.drain() {
            frames.push(play(PlayPacket::SetCooldown(SetCooldown {
                cooldown_group,
                duration: 0,
            })));
        }
        if std::mem::take(&mut self.border) {
            frames.push(play(PlayPacket::InitializeBorder(InitializeBorder {
                center_x: 0.0,
                center_z: 0.0,
                old_diameter: FRESH_BORDER_DIAMETER,
                new_diameter: FRESH_BORDER_DIAMETER,
                lerp_ticks: 0,
                portal_teleport_boundary: FRESH_PORTAL_BOUNDARY,
                warning_blocks: 5,
                warning_ticks: 15,
            })));
        }
        if std::mem::take(&mut self.rain) {
            for (event, value) in [
                (GameEventType::EndRaining, 0.0),
                (GameEventType::RainLevelChange, 0.0),
                (GameEventType::ThunderLevelChange, 0.0),
            ] {
                frames.push(play(PlayPacket::GameEvent(GameEvent { event, value })));
            }
        }
        if std::mem::take(&mut self.tick_rate) {
            frames.push(frame(cb::TICKING_STATE, |body| {
                20.0f32.encode(body);
                false.encode(body);
            }));
        }
        if std::mem::take(&mut self.chat_completions) {
            frames.push(frame(cb::CUSTOM_CHAT_COMPLETIONS, |body| {
                const SET: i32 = 2;
                VarI32(SET).encode(body);
                VarI32(0).encode(body);
            }));
        }
        if std::mem::take(&mut self.advancements) {
            frames.push(frame(cb::UPDATE_ADVANCEMENTS, |body| {
                true.encode(body);
                VarI32(0).encode(body);
                VarI32(0).encode(body);
                VarI32(0).encode(body);
                false.encode(body);
            }));
        }
        if std::mem::take(&mut self.recipes) {
            frames.push(frame(cb::RECIPE_BOOK_ADD, |body| {
                VarI32(0).encode(body);
                true.encode(body);
            }));
        }
        if std::mem::take(&mut self.health) {
            frames.push(frame(cb::SET_HEALTH, |body| {
                20.0f32.encode(body);
                VarI32(20).encode(body);
                5.0f32.encode(body);
            }));
        }
        if std::mem::take(&mut self.experience) {
            frames.push(frame(cb::SET_EXPERIENCE, |body| {
                0.0f32.encode(body);
                VarI32(0).encode(body);
                VarI32(0).encode(body);
            }));
        }
        if std::mem::take(&mut self.inventory) {
            frames.push(play(PlayPacket::SetContainerContent(SetContainerContent {
                container_id: 0,
                state_id: 0,
                items: vec![Slot::EMPTY; PLAYER_MENU_SLOTS],
                carried: Slot::EMPTY,
            })));
        }
        if std::mem::take(&mut self.cursor) {
            frames.push(play(PlayPacket::SetCursorItem(SetCursorItem {
                contents: Slot::EMPTY,
            })));
        }
        frames.push(play(PlayPacket::SetHeldSlot(SetHeldSlot { slot: 0 })));

        if self.game_mode != login.game_mode {
            frames.push(play(PlayPacket::GameEvent(GameEvent {
                event: GameEventType::ChangeGameMode,
                value: f32::from(login.game_mode),
            })));
        }
        let fresh = Abilities::of_game_mode(login.game_mode);
        if self.game_mode != login.game_mode || self.abilities.is_some_and(|held| held != fresh) {
            frames.push(play(PlayPacket::PlayerAbilities(PlayerAbilities {
                flags: fresh.flags,
                flying_speed: fresh.flying_speed,
                walking_speed: fresh.walking_speed,
            })));
        }
        if let Some(standing) = &self.standing {
            if standing.view_distance != login.view_distance {
                frames.push(frame(cb::SET_CHUNK_CACHE_RADIUS, |body| {
                    VarI32(login.view_distance).encode(body);
                }));
            }
            if standing.simulation_distance != login.simulation_distance {
                frames.push(frame(cb::SET_SIMULATION_DISTANCE, |body| {
                    VarI32(login.simulation_distance).encode(body);
                }));
            }
        }

        self.stale_chunks.extend(self.chunks.drain());
        self.note_login(login);
        frames
    }

    pub fn menu_closed(&mut self) {
        self.menu = None;
    }

    /// The chunks of the last server that the new one did not send again by now.
    pub fn take_stale_chunks(&mut self) -> Vec<Bytes> {
        self.stale_chunks
            .drain()
            .map(|(chunk_x, chunk_z)| {
                frame(cb::FORGET_LEVEL_CHUNK, |body| {
                    UnloadChunk { chunk_z, chunk_x }.encode(body);
                })
            })
            .collect()
    }

    /// A client that went through a reconfiguration holds nothing any more.
    pub fn forget(&mut self) {
        *self = Self::default();
    }

    pub fn tally(&self) -> Tally {
        Tally {
            entities: self.entities.len(),
            chunks: self.chunks.len(),
            boss_bars: self.boss_bars.len(),
            objectives: self.objectives.len(),
            teams: self.teams.len(),
            tab_entries: self.tab_entries.len(),
            effects: self.effects.len(),
            bytes: self.heap_bytes(),
        }
    }

    /// The bytes held by the id sets, leaving out what the allocator rounds up.
    pub fn heap_bytes(&self) -> usize {
        fn set<T>(set: &HashSet<T>) -> usize {
            set.capacity() * (size_of::<T>() + 1)
        }
        let names = |names: &HashSet<String>| {
            set(names) + names.iter().map(String::capacity).sum::<usize>()
        };
        set(&self.entities)
            + set(&self.chunks)
            + set(&self.stale_chunks)
            + set(&self.boss_bars)
            + names(&self.objectives)
            + names(&self.teams)
            + set(&self.tab_entries)
            + set(&self.effects)
            + set(&self.attributes)
            + names(&self.cooldowns)
    }
}

/// A plain text component, the way the client reads it from a play packet.
pub fn text(content: &str) -> Nbt {
    Nbt {
        name: "".into(),
        compound: vec![("text".into(), Tag::String(content.into()))].into(),
    }
}

fn skip_bytes(packet: &mut Decoder<'_>) -> Result<(), DecodeError> {
    let len = packet.decode::<VarI32>()?.0;
    let len = usize::try_from(len).map_err(|_| DecodeError::InvalidLength)?;
    packet.take(len)?;
    Ok(())
}

/// `Entity` data at its defaults. What a player gets from `LivingEntity` and `Avatar` is left
/// out: a client computes it, or only shows it to others.
fn fresh_player_data(entity_id: i32) -> SetEntityData {
    let entry = |index, value| EntityMetadataEntry { index, value };
    SetEntityData {
        entity_id,
        entries: vec![
            entry(0, EntityMetadataValue::Byte(0)),
            entry(1, EntityMetadataValue::Int(300)),
            entry(2, EntityMetadataValue::OptionalComponent(None)),
            entry(3, EntityMetadataValue::Boolean(false)),
            entry(4, EntityMetadataValue::Boolean(false)),
            entry(5, EntityMetadataValue::Boolean(false)),
            entry(6, EntityMetadataValue::Pose(0)),
            entry(7, EntityMetadataValue::Int(0)),
        ],
    }
}

#[cfg(test)]
mod tests {
    use voidmc_data::{Version, packets};
    use voidmc_protocol::clientbound::{ObjectiveInfo, RenderType, TeamParameters};

    use super::*;

    const PLAYER: i32 = 7;
    const PROFILE: Uuid = Uuid::from_u128(77);

    fn login(entity_id: i32) -> Login {
        Login {
            entity_id,
            is_hardcore: false,
            dimension_names: vec!["minecraft:overworld".to_string()],
            max_players: 10,
            view_distance: 10,
            simulation_distance: 10,
            reduced_debug_info: false,
            enable_respawn_screen: true,
            do_limited_crafting: false,
            dimension_type: 0,
            dimension_name: "minecraft:overworld".to_string(),
            hashed_seed: 0,
            game_mode: 1,
            previous_game_mode: -1,
            is_debug: false,
            is_flat: false,
            last_death_location: None,
            portal_cooldown: 0,
            sea_level: 63,
            enforces_secure_chat: false,
        }
    }

    fn joined() -> Ledger {
        let mut ledger = Ledger::default();
        ledger.observe(&tagged(&PlayPacket::Login(login(PLAYER))));
        ledger
    }

    fn id_of(frame: &[u8]) -> i32 {
        split(frame).unwrap().0
    }

    fn add_entity(id: i32) -> Bytes {
        frame(cb::ADD_ENTITY, |body| VarI32(id).encode(body))
    }

    fn boss_bar(id: Uuid) -> Bytes {
        tagged(&PlayPacket::BossEvent(BossEvent {
            id,
            action: BossEventAction::Add {
                title: text("bar"),
                progress: 1.0,
                color: Default::default(),
                division: Default::default(),
                flags: Default::default(),
            },
        }))
    }

    #[test]
    fn every_packet_of_the_protocol_is_in_the_ledger() {
        for (name, id) in packets(Version::V26_1_2, "play", "clientbound").unwrap() {
            let mut ledger = Ledger::default();
            ledger.observe(&frame(*id, |_| {}));
            if let Some(reason) = ledger.tainted() {
                assert!(
                    !reason.contains("is not in the ledger"),
                    "{name} has no place in the ledger"
                );
            }
        }
    }

    #[test]
    fn a_packet_cut_short_taints_the_ledger_and_panics_nothing() {
        let mut ledger = joined();
        ledger.observe(&frame(cb::BOSS_EVENT, |body| body.push(1)));
        assert!(ledger.tainted().unwrap().contains("could not be read"));
        assert!(ledger.fits(&login(PLAYER)).is_err());
    }

    #[test]
    fn garbage_never_panics() {
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        };
        let mut ledger = joined();
        for _ in 0..20_000 {
            let len = usize::from(next() % 64);
            let bytes: Vec<u8> = (0..len).map(|_| next()).collect();
            ledger.observe(&bytes);
        }
    }

    #[test]
    fn entities_come_back_out_except_the_player() {
        let mut ledger = joined();
        ledger.observe(&add_entity(1));
        ledger.observe(&add_entity(2));
        ledger.observe(&add_entity(PLAYER));
        ledger.observe(&tagged(&ManualPlayPacket::RemoveEntities(RemoveEntities {
            entity_ids: vec![2],
        })));
        let frames = ledger.teardown(&login(PLAYER), PROFILE);
        let removal = frames
            .iter()
            .find(|frame| id_of(frame) == cb::REMOVE_ENTITIES)
            .unwrap();
        assert_eq!(
            removal[..],
            tagged(&ManualPlayPacket::RemoveEntities(RemoveEntities {
                entity_ids: vec![1],
            }))[..]
        );
        assert_eq!(ledger.tally().entities, 0);
    }

    #[test]
    fn a_boss_bar_removed_by_the_server_is_not_removed_twice() {
        let (gone, stays) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let mut ledger = joined();
        ledger.observe(&boss_bar(gone));
        ledger.observe(&boss_bar(stays));
        ledger.observe(&tagged(&PlayPacket::BossEvent(BossEvent {
            id: gone,
            action: BossEventAction::Remove,
        })));
        let frames = ledger.teardown(&login(PLAYER), PROFILE);
        let removed: Vec<_> = frames
            .iter()
            .filter(|frame| id_of(frame) == cb::BOSS_EVENT)
            .collect();
        assert_eq!(removed.len(), 1);
        assert_eq!(
            removed[0][..],
            tagged(&PlayPacket::BossEvent(BossEvent {
                id: stays,
                action: BossEventAction::Remove,
            }))[..]
        );
    }

    #[test]
    fn objectives_and_teams_are_removed_by_name() {
        let mut ledger = joined();
        ledger.observe(&tagged(&PlayPacket::SetObjective(SetObjective {
            name: "sidebar".to_string(),
            action: ObjectiveAction::Create(ObjectiveInfo {
                display_name: text("x"),
                render_type: RenderType::Integer,
                number_format: None,
            }),
        })));
        ledger.observe(&tagged(&PlayPacket::SetPlayerTeam(SetPlayerTeam {
            name: "red".to_string(),
            action: TeamAction::Create {
                parameters: TeamParameters {
                    display_name: text("x"),
                    flags: Default::default(),
                    name_tag_visibility: Default::default(),
                    collision_rule: Default::default(),
                    color: Default::default(),
                    prefix: text(""),
                    suffix: text(""),
                },
                entities: vec!["Bot".to_string()],
            },
        })));
        assert_eq!(ledger.tally().objectives, 1);
        assert_eq!(ledger.tally().teams, 1);
        let frames = ledger.teardown(&login(PLAYER), PROFILE);
        assert!(frames.iter().any(|f| id_of(f) == cb::SET_OBJECTIVE));
        assert!(frames.iter().any(|f| id_of(f) == cb::SET_PLAYER_TEAM));
    }

    #[test]
    fn tab_entries_are_read_through_their_whole_layout() {
        let (first, second) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let entry = |uuid| PlayerInfoEntry {
            uuid,
            name: "Bot".to_string(),
            listed: true,
            display_name: Some(text("Bot")),
            ..Default::default()
        };
        let mut ledger = joined();
        ledger.observe(&tagged(&ManualPlayPacket::PlayerInfoUpdate(
            PlayerInfoUpdate {
                actions: PlayerInfoActions::INITIALIZING,
                entries: vec![entry(first), entry(second)],
            },
        )));
        assert!(ledger.tainted().is_none());
        assert_eq!(ledger.tally().tab_entries, 2);
        ledger.observe(&tagged(&ManualPlayPacket::PlayerInfoRemove(
            PlayerInfoRemove {
                uuids: vec![second],
            },
        )));
        assert_eq!(ledger.tally().tab_entries, 1);
    }

    #[test]
    fn the_entry_of_the_player_stays_and_follows_the_new_game_mode() {
        let other = Uuid::from_u128(1);
        let entry = |uuid, game_mode| PlayerInfoEntry {
            uuid,
            name: "Bot".to_string(),
            game_mode,
            ..Default::default()
        };
        let mut ledger = joined();
        ledger.observe(&tagged(&ManualPlayPacket::PlayerInfoUpdate(
            PlayerInfoUpdate {
                actions: PlayerInfoActions::INITIALIZING,
                entries: vec![entry(PROFILE, 3), entry(other, 3)],
            },
        )));

        let frames = ledger.teardown(&login(PLAYER), PROFILE);

        let removed = frames
            .iter()
            .find(|f| id_of(f) == cb::PLAYER_INFO_REMOVE)
            .unwrap();
        let expected = tagged(&ManualPlayPacket::PlayerInfoRemove(PlayerInfoRemove {
            uuids: vec![other],
        }));
        assert_eq!(removed, &expected);
        let updated = frames
            .iter()
            .find(|f| id_of(f) == cb::PLAYER_INFO_UPDATE)
            .unwrap();
        let expected = tagged(&ManualPlayPacket::PlayerInfoUpdate(
            PlayerInfoUpdate::single(PlayerInfoActions::UPDATE_GAME_MODE, entry(PROFILE, 1)),
        ));
        assert_eq!(updated, &expected);
        assert_eq!(ledger.tally().tab_entries, 1);
    }

    #[test]
    fn distances_changed_during_the_run_go_back_to_those_of_the_login() {
        let mut ledger = joined();
        ledger.observe(&frame(cb::SET_CHUNK_CACHE_RADIUS, |body| {
            VarI32(21).encode(body);
        }));
        ledger.observe(&frame(cb::SET_SIMULATION_DISTANCE, |body| {
            VarI32(3).encode(body);
        }));

        let frames = ledger.teardown(&login(PLAYER), PROFILE);

        let radius = frame(cb::SET_CHUNK_CACHE_RADIUS, |body| VarI32(10).encode(body));
        let simulation = frame(cb::SET_SIMULATION_DISTANCE, |body| VarI32(10).encode(body));
        assert!(frames.contains(&radius));
        assert!(frames.contains(&simulation));
    }

    #[test]
    fn only_the_effects_of_the_player_are_kept() {
        let mut ledger = joined();
        let effect = |entity: i32, effect: i32| {
            frame(cb::UPDATE_MOB_EFFECT, |body| {
                VarI32(entity).encode(body);
                VarI32(effect).encode(body);
            })
        };
        ledger.observe(&effect(PLAYER, 3));
        ledger.observe(&effect(PLAYER + 1, 4));
        let frames = ledger.teardown(&login(PLAYER), PROFILE);
        let removed: Vec<_> = frames
            .iter()
            .filter(|f| id_of(f) == cb::REMOVE_MOB_EFFECT)
            .collect();
        assert_eq!(removed.len(), 1);
    }

    #[test]
    fn the_camera_comes_home_before_its_entity_goes() {
        let mut ledger = joined();
        ledger.observe(&add_entity(40));
        ledger.observe(&tagged(&PlayPacket::SetCamera(SetCamera { camera_id: 40 })));
        let frames = ledger.teardown(&login(PLAYER), PROFILE);
        let at = |id| frames.iter().position(|frame| id_of(frame) == id).unwrap();
        assert!(at(cb::SET_CAMERA) < at(cb::REMOVE_ENTITIES));
        assert_eq!(
            frames[at(cb::SET_CAMERA)][..],
            tagged(&PlayPacket::SetCamera(SetCamera { camera_id: PLAYER }))[..]
        );
    }

    #[test]
    fn a_camera_back_on_the_player_needs_nothing() {
        let mut ledger = joined();
        ledger.observe(&tagged(&PlayPacket::SetCamera(SetCamera { camera_id: 40 })));
        ledger.observe(&tagged(&PlayPacket::SetCamera(SetCamera {
            camera_id: PLAYER,
        })));
        let frames = ledger.teardown(&login(PLAYER), PROFILE);
        assert!(!frames.iter().any(|frame| id_of(frame) == cb::SET_CAMERA));
    }

    #[test]
    fn the_player_leaves_its_vehicle_before_the_vehicle_goes() {
        let mut ledger = joined();
        ledger.observe(&add_entity(40));
        ledger.observe(&tagged(&ManualPlayPacket::SetPassengers(SetPassengers {
            entity_id: 40,
            passengers: vec![PLAYER],
        })));
        let frames = ledger.teardown(&login(PLAYER), PROFILE);
        let at = |id| frames.iter().position(|frame| id_of(frame) == id).unwrap();
        assert!(at(cb::SET_PASSENGERS) < at(cb::REMOVE_ENTITIES));
    }

    #[test]
    fn a_new_game_mode_comes_with_its_abilities() {
        let mut ledger = joined();
        ledger.observe(&tagged(&PlayPacket::GameEvent(GameEvent {
            event: GameEventType::ChangeGameMode,
            value: 3.0,
        })));
        let frames = ledger.teardown(&login(PLAYER), PROFILE);
        let at = |id| frames.iter().position(|frame| id_of(frame) == id).unwrap();
        assert!(at(cb::GAME_EVENT) < at(cb::PLAYER_ABILITIES));
    }

    #[test]
    fn same_mode_same_abilities_send_nothing() {
        let mut ledger = joined();
        let frames = ledger.teardown(&login(PLAYER), PROFILE);
        assert!(!frames.iter().any(|f| id_of(f) == cb::GAME_EVENT));
        assert!(!frames.iter().any(|f| id_of(f) == cb::PLAYER_ABILITIES));
    }

    #[test]
    fn custom_abilities_go_back_to_those_of_the_mode() {
        let mut ledger = joined();
        ledger.observe(&tagged(&PlayPacket::PlayerAbilities(PlayerAbilities {
            flags: 0,
            flying_speed: 0.2,
            walking_speed: 0.1,
        })));
        let frames = ledger.teardown(&login(PLAYER), PROFILE);
        assert!(frames.iter().any(|f| id_of(f) == cb::PLAYER_ABILITIES));
    }

    #[test]
    fn rain_stops_and_the_border_returns() {
        let mut ledger = joined();
        ledger.observe(&tagged(&PlayPacket::GameEvent(GameEvent {
            event: GameEventType::BeginRaining,
            value: 0.0,
        })));
        ledger.observe(&tagged(&PlayPacket::SetBorderSize(
            voidmc_protocol::clientbound::SetBorderSize { diameter: 10.0 },
        )));
        let frames = ledger.teardown(&login(PLAYER), PROFILE);
        assert!(frames.iter().any(|f| id_of(f) == cb::INITIALIZE_BORDER));
        assert_eq!(
            frames.iter().filter(|f| id_of(f) == cb::GAME_EVENT).count(),
            3
        );
    }

    #[test]
    fn a_teardown_leaves_nothing_to_tear_down() {
        let mut ledger = joined();
        ledger.observe(&add_entity(1));
        ledger.observe(&boss_bar(Uuid::from_u128(1)));
        ledger.observe(&frame(cb::SET_HEALTH, |_| {}));
        ledger.teardown(&login(PLAYER), PROFILE);
        let second = ledger.teardown(&login(PLAYER), PROFILE);
        let ids: Vec<i32> = second.iter().map(|frame| id_of(frame)).collect();
        assert_eq!(ids, vec![cb::CLEAR_TITLES, cb::SET_HELD_SLOT]);
    }

    #[test]
    fn old_chunks_stay_until_the_new_server_had_its_chance() {
        let chunk = |x: i32, z: i32| {
            frame(cb::LEVEL_CHUNK_WITH_LIGHT, |body| {
                x.encode(body);
                z.encode(body);
            })
        };
        let mut ledger = joined();
        ledger.observe(&chunk(0, 0));
        ledger.observe(&chunk(1, 2));
        ledger.observe(&frame(cb::FORGET_LEVEL_CHUNK, |body| {
            UnloadChunk {
                chunk_z: 0,
                chunk_x: 0,
            }
            .encode(body);
        }));
        ledger.observe(&chunk(5, 5));
        let frames = ledger.teardown(&login(PLAYER), PROFILE);
        assert!(!frames.iter().any(|f| id_of(f) == cb::FORGET_LEVEL_CHUNK));
        ledger.observe(&chunk(5, 5));
        let forgotten = ledger.take_stale_chunks();
        assert_eq!(forgotten.len(), 1);
        let expected = frame(cb::FORGET_LEVEL_CHUNK, |body| {
            UnloadChunk {
                chunk_z: 2,
                chunk_x: 1,
            }
            .encode(body);
        });
        assert_eq!(forgotten[0][..], expected[..]);
        assert_eq!(ledger.tally().chunks, 1);
    }

    #[test]
    fn a_bundle_cut_by_the_crash_is_closed() {
        let mut ledger = joined();
        assert!(ledger.close_bundle().is_none());
        ledger.observe(&frame(cb::BUNDLE_DELIMITER, |_| {}));
        assert!(ledger.close_bundle().is_some());
        assert!(ledger.close_bundle().is_none());
        ledger.observe(&frame(cb::BUNDLE_DELIMITER, |_| {}));
        ledger.observe(&frame(cb::BUNDLE_DELIMITER, |_| {}));
        assert!(ledger.close_bundle().is_none());
    }

    #[test]
    fn another_id_does_not_fit_and_says_how_to_fix_it() {
        let reason = joined().fits(&login(PLAYER + 1)).unwrap_err();
        assert!(reason.contains("VOIDMC_STABLE_PLAYER_IDS"));
    }

    #[test]
    fn another_level_does_not_fit() {
        let ledger = joined();
        let mut other = login(PLAYER);
        other.dimension_name = "minecraft:the_nether".to_string();
        assert!(ledger.fits(&other).is_err());
        let mut other = login(PLAYER);
        other.game_mode = 0;
        other.view_distance = 4;
        assert!(ledger.fits(&other).is_ok());
    }

    #[test]
    fn a_respawn_moves_the_level_the_ledger_compares() {
        let mut ledger = joined();
        ledger.observe(&add_entity(1));
        ledger.observe(&tagged(&PlayPacket::Respawn(Respawn {
            dimension_type: 1,
            dimension_name: "minecraft:the_nether".to_string(),
            hashed_seed: 0,
            game_mode: 1,
            previous_game_mode: -1,
            is_debug: false,
            is_flat: false,
            last_death_location: None,
            portal_cooldown: 0,
            sea_level: 63,
            data_to_keep: 0,
        })));
        assert_eq!(ledger.tally().entities, 0);
        assert!(ledger.fits(&login(PLAYER)).is_err());
    }

    #[test]
    fn what_cannot_be_undone_forces_a_reconfiguration() {
        for id in [
            cb::WAYPOINT,
            cb::BLOCK_DESTRUCTION,
            cb::OPEN_BOOK,
            cb::OPEN_SIGN_EDITOR,
            cb::PLAYER_COMBAT_KILL,
        ] {
            let mut ledger = joined();
            ledger.observe(&frame(id, |_| {}));
            assert!(ledger.fits(&login(PLAYER)).is_err(), "{id:#x}");
        }
    }

    #[test]
    fn game_events_that_stay_on_the_client_force_a_reconfiguration() {
        for event in [
            GameEventType::WinGame,
            GameEventType::EnableRespawnScreen,
            GameEventType::LimitedCrafting,
        ] {
            let mut ledger = joined();
            ledger.observe(&tagged(&PlayPacket::GameEvent(GameEvent {
                event,
                value: 1.0,
            })));
            assert!(ledger.fits(&login(PLAYER)).is_err(), "{event:?}");
        }
    }

    #[test]
    fn a_chat_session_in_a_tab_entry_is_read_to_its_end() {
        let mut ledger = joined();
        ledger.observe(&frame(cb::PLAYER_INFO_UPDATE, |body| {
            0x07u8.encode(body);
            VarI32(2).encode(body);
            for uuid in [Uuid::from_u128(1), Uuid::from_u128(2)] {
                uuid.encode(body);
                "Bot".to_string().encode(body);
                VarI32(0).encode(body);
                true.encode(body);
                Uuid::from_u128(9).encode(body);
                0i64.encode(body);
                VarI32(3).encode(body);
                body.extend_from_slice(&[1, 2, 3]);
                VarI32(2).encode(body);
                body.extend_from_slice(&[4, 5]);
                VarI32(1).encode(body);
            }
        }));
        assert!(ledger.tainted().is_none());
        assert_eq!(ledger.tally().tab_entries, 2);
    }

    #[test]
    fn a_client_that_never_logged_in_does_not_fit() {
        assert!(Ledger::default().fits(&login(1)).is_err());
    }
}
