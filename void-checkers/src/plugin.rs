use bevy_app::{App, Plugin, Update};
use bevy_ecs::prelude::*;
use uuid::Uuid;
use voidmc::{
    BlockDisplay, Command, CommandBuilder, CommandContext, CommandRegistry, Display,
    DisplayTransform, EntityBuilder, EntityKind, Glowing, ItemDisplay, ItemDisplayContext,
    ItemStack, Messages, Particle, Particles, Passengers, PlayerAbilities, Players, Sound, Sounds,
    Teleport, TeleportOutcome, TextColor,
    components::{Position, Rotation, ServerControlledPosition},
    events::{PlayerQuitEvent, PlayerReadyEvent, PlayerSwingArmEvent, PlayerTeleportEvent},
    messages::text_component,
};
use voidmc_data::v26_1_2::blocks as b;
use voidmc_protocol::clientbound::ResourcePackPush;

use crate::game::{Color, Game, Outcome, Piece, SIZE, Square, Step};
use crate::geometry::{self, BOARD_Y};

const PACK_ID: Uuid = Uuid::from_u128(0x766f_6964_6d63_4000_8000_6368_6563_6b72);

/// Height of a resting piece's entity: item models are centred on it.
const REST_Y: f64 = BOARD_Y + 0.5;
const HOVER_LIFT: f64 = 0.12;
const HOLD_LIFT: f64 = 0.9;
const BOB: f64 = 0.06;
const FOLLOW_TICKS: u8 = 2;
const HOP_TICKS: u64 = 8;
const RETURN_TICKS: u64 = 5;
const FADE_TICKS: u64 = 6;
const STATUS_PERIOD: u64 = 30;
const GHOST_PULSE: u64 = 10;
const GLOW: i32 = 0xFFE9A0;
const VIEW_RANGE: f32 = 4.0;

/// Where clients download the resource pack from.
#[derive(Resource)]
pub struct PackUrl(pub String);

#[derive(Clone, Copy)]
struct Token {
    entity: Entity,
    square: Square,
}

enum Anim {
    Hop {
        entity: Entity,
        from: [f64; 3],
        to: [f64; 3],
        start: u64,
        ticks: u64,
        height: f64,
        step: Option<Step>,
    },
    Fade {
        entity: Entity,
        start: u64,
    },
}

/// The one game on the server, and everything drawn for it.
#[derive(Resource, Default)]
pub struct Table {
    game: Game,
    tokens: Vec<Token>,
    held: Option<Entity>,
    aim: Option<(f64, f64)>,
    hover: Option<Square>,
    glowing: Option<Entity>,
    anims: Vec<Anim>,
    ghost: Option<Entity>,
    markers: Vec<Entity>,
    seat: Option<Entity>,
    player: Option<Entity>,
    /// Landings of this tick, played by [`play_effects`].
    landed: Vec<Step>,
    tick: u64,
    reset: bool,
}

pub struct CheckersPlugin {
    pub pack_url: String,
}

impl Plugin for CheckersPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(PackUrl(self.pack_url.clone()))
            .init_resource::<Table>()
            .add_observer(seat_player)
            .add_observer(player_seated)
            .add_observer(player_left)
            .add_observer(click)
            .add_systems(Update, (tick, play_effects).chain());
        app.world_mut()
            .resource_mut::<CommandRegistry>()
            .register(checkers_command());
    }
}

fn piece_item(piece: Piece) -> ItemStack {
    let name = match (piece.color, piece.king) {
        (Color::White, false) => "minecraft:white_dye",
        (Color::White, true) => "minecraft:yellow_dye",
        (Color::Black, false) => "minecraft:black_dye",
        (Color::Black, true) => "minecraft:orange_dye",
    };
    ItemStack::of(name, 1).unwrap_or(ItemStack::EMPTY)
}

fn ghost_item(king: bool) -> ItemStack {
    let name = if king {
        "minecraft:cyan_dye"
    } else {
        "minecraft:light_blue_dye"
    };
    ItemStack::of(name, 1).unwrap_or(ItemStack::EMPTY)
}

fn display(transform: DisplayTransform) -> Display {
    Display::default()
        .transform(transform)
        .interpolation_ticks(u16::from(FOLLOW_TICKS))
        .teleport_ticks(FOLLOW_TICKS)
        .brightness(15, 15)
        .view_range(VIEW_RANGE)
        .culling_box(2.0, 2.0)
        .glow_color(GLOW)
}

fn spawn_item(commands: &mut Commands, at: [f64; 3], item: ItemStack, scale: f32) -> Entity {
    EntityBuilder::new(EntityKind::ItemDisplay)
        .at(at[0], at[1], at[2])
        .with_bundle((
            display(DisplayTransform::default().uniform_scale(scale)),
            ItemDisplay::new(item).context(ItemDisplayContext::None),
        ))
        .spawn(commands)
        .id()
}

fn spawn_slab(commands: &mut Commands, at: [f64; 3], block: i32, size: [f32; 3]) -> Entity {
    EntityBuilder::new(EntityKind::BlockDisplay)
        .at(at[0], at[1], at[2])
        .with_bundle((
            display(DisplayTransform::default().scale(size[0], size[1], size[2]))
                .culling_box(size[0].max(size[2]) * 2.0, 2.0),
            BlockDisplay(block),
        ))
        .spawn(commands)
        .id()
}

fn rest(square: Square) -> [f64; 3] {
    let (x, z) = geometry::centre(square);
    [x, REST_Y, z]
}

fn build_board(commands: &mut Commands) {
    let size = f32::from(SIZE);
    spawn_slab(
        commands,
        [-0.5, BOARD_Y - 0.35, -0.5],
        b::DARK_OAK_PLANKS,
        [size + 1.0, 0.2, size + 1.0],
    );
    for row in 0..SIZE {
        for col in 0..SIZE {
            let block = if Square::new(row, col).is_some() {
                b::SPRUCE_PLANKS
            } else {
                b::BIRCH_PLANKS
            };
            let at = [f64::from(col), BOARD_Y - 0.15, f64::from(row)];
            spawn_slab(commands, at, block, [1.0, 0.15, 1.0]);
        }
    }
}

impl Table {
    fn deal(&mut self, commands: &mut Commands) {
        self.game = Game::default();
        self.held = None;
        self.glowing = None;
        self.hover = None;
        for token in self.tokens.drain(..) {
            commands.entity(token.entity).despawn();
        }
        for anim in self.anims.drain(..) {
            if let Anim::Fade { entity, .. } = anim {
                commands.entity(entity).despawn();
            }
        }
        for index in 0..SIZE * SIZE {
            let Some(square) = Square::from_index(index) else {
                continue;
            };
            if let Some(piece) = self.game.piece(square) {
                let entity = spawn_item(commands, rest(square), piece_item(piece), 1.0);
                self.tokens.push(Token { entity, square });
            }
        }
        self.refresh_markers(commands);
    }

    fn refresh_markers(&mut self, commands: &mut Commands) {
        for marker in self.markers.drain(..) {
            commands.entity(marker).despawn();
        }
        for square in self.game.targets() {
            let (x, z) = geometry::centre(square);
            let at = [x - 0.4, BOARD_Y + 0.005, z - 0.4];
            let marker = spawn_slab(commands, at, b::LIME_STAINED_GLASS, [0.8, 0.02, 0.8]);
            self.markers.push(marker);
        }
    }

    fn token_at(&self, square: Square) -> Option<Token> {
        self.tokens.iter().copied().find(|t| t.square == square)
    }

    fn status(&self) -> String {
        let side = |color| match color {
            Color::White => "Blancs",
            Color::Black => "Noirs",
        };
        match self.game.winner() {
            Some(winner) => format!("{} gagnent ! /dames pour rejouer", side(winner)),
            None if self.game.must_capture() => {
                format!("Trait aux {} : prise obligatoire", side(self.game.turn()))
            }
            None => format!("Trait aux {}", side(self.game.turn())),
        }
    }
}

fn seat_player(
    event: On<PlayerReadyEvent>,
    mut table: ResMut<Table>,
    mut commands: Commands,
    players: Players,
    url: Res<PackUrl>,
) {
    let player = event.entity;
    players.send(
        player,
        ResourcePackPush {
            id: PACK_ID,
            url: url.0.clone(),
            hash: String::new(),
            required: false,
            prompt: Some(text_component(
                "Les pions du plateau utilisent ce pack.",
                TextColor::Yellow,
            )),
        },
    );
    if table.player.is_some() {
        return;
    }
    if table.seat.is_none() {
        build_board(&mut commands);
        let [x, y, z] = geometry::seat();
        let seat = EntityBuilder::new(EntityKind::ItemDisplay)
            .at(x, y, z)
            .with(Passengers::default())
            .spawn(&mut commands)
            .id();
        table.seat = Some(seat);
        let ghost = spawn_item(&mut commands, [4.0, REST_Y, 4.0], ghost_item(false), 0.0);
        table.ghost = Some(ghost);
        table.deal(&mut commands);
    }
    table.player = Some(player);
    let [x, y, z] = geometry::seat();
    commands.entity(player).insert((
        ServerControlledPosition,
        PlayerAbilities::new().invulnerable(true).flying(true),
        Teleport::to(x, y, z).facing(geometry::YAW, geometry::PITCH),
    ));
}

fn player_seated(
    event: On<PlayerTeleportEvent>,
    table: Res<Table>,
    mut seats: Query<&mut Passengers>,
) {
    if table.player != Some(event.entity) || event.outcome != TeleportOutcome::Confirmed {
        return;
    }
    if let Some(mut seat) = table.seat.and_then(|seat| seats.get_mut(seat).ok()) {
        seat.push(event.entity);
    }
}

fn player_left(event: On<PlayerQuitEvent>, mut table: ResMut<Table>) {
    if table.player == Some(event.entity) {
        table.player = None;
    }
}

fn click(
    event: On<PlayerSwingArmEvent>,
    mut table: ResMut<Table>,
    mut commands: Commands,
    positions: Query<&Position>,
    sounds: Sounds,
) {
    let player = event.entity;
    if table.player != Some(player) || !table.anims.is_empty() {
        return;
    }
    let table = &mut *table;
    let Some(held) = table.held else {
        let Some(square) = table.hover else {
            return;
        };
        if table.game.click(square) == Outcome::Grabbed {
            table.held = table.token_at(square).map(|token| token.entity);
            table.refresh_markers(&mut commands);
            sounds.play_to(player, Sound::new("item.armor.equip_leather").pitch(1.4));
        } else {
            sounds.play_to(
                player,
                Sound::new("block.wooden_button.click_off").volume(0.5),
            );
        }
        return;
    };
    let Some(from) = table.game.selected() else {
        return;
    };
    let Ok(current) = positions.get(held) else {
        return;
    };
    let current = [current.x, current.y, current.z];
    let target = table
        .hover
        .filter(|square| table.game.targets().contains(square));
    let square = match target {
        Some(square) => square,
        None if table.game.chaining() => return,
        None => from,
    };
    match table.game.click(square) {
        Outcome::Released => {
            table.held = None;
            table.anims.push(Anim::Hop {
                entity: held,
                from: current,
                to: rest(from),
                start: table.tick,
                ticks: RETURN_TICKS,
                height: 0.1,
                step: None,
            });
            sounds.play_to(
                player,
                Sound::new("block.wood.place").volume(0.5).pitch(1.5),
            );
        }
        Outcome::Moved(step) => {
            table.held = None;
            if let Some(token) = table.tokens.iter_mut().find(|t| t.entity == held) {
                token.square = step.to;
            }
            if let Some(captured) = step.captured
                && let Some(index) = table.tokens.iter().position(|t| t.square == captured)
            {
                let entity = table.tokens.remove(index).entity;
                table.anims.push(Anim::Fade {
                    entity,
                    start: table.tick + HOP_TICKS / 2,
                });
            }
            table.anims.push(Anim::Hop {
                entity: held,
                from: current,
                to: rest(step.to),
                start: table.tick,
                ticks: HOP_TICKS,
                height: if step.captured.is_some() { 0.9 } else { 0.35 },
                step: Some(step),
            });
        }
        Outcome::Grabbed | Outcome::Ignored => {}
    }
    table.refresh_markers(&mut commands);
}

fn set_position(position: &mut Mut<Position>, at: [f64; 3]) {
    let at = Position {
        x: at[0],
        y: at[1],
        z: at[2],
    };
    if **position != at {
        **position = at;
    }
}

type Visuals<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut Position,
        &'static mut Display,
        &'static mut ItemDisplay,
    ),
>;

fn run_anims(table: &mut Table, visuals: &mut Visuals, commands: &mut Commands) {
    let now = table.tick;
    let mut landed = Vec::new();
    table.anims.retain(|anim| match *anim {
        Anim::Hop {
            entity,
            from,
            to,
            start,
            ticks,
            height,
            step,
        } => {
            let t = ((now - start) as f64 / ticks as f64).min(1.0);
            if let Ok((mut position, _, _)) = visuals.get_mut(entity) {
                set_position(&mut position, geometry::arc(from, to, t, height));
            }
            if t < 1.0 {
                return true;
            }
            if let Some(step) = step {
                landed.push((entity, step));
            }
            false
        }
        Anim::Fade { entity, start } => {
            if now == start
                && let Ok((_, mut display, _)) = visuals.get_mut(entity)
            {
                display.interpolation_ticks = FADE_TICKS as u16;
                display.transform = DisplayTransform::default()
                    .uniform_scale(0.0)
                    .translation(0.0, 0.8, 0.0);
            }
            if now < start + FADE_TICKS {
                return true;
            }
            commands.entity(entity).despawn();
            false
        }
    });
    for (entity, step) in landed {
        if step.promoted
            && let Some(piece) = table.game.piece(step.to)
            && let Ok((_, _, mut item)) = visuals.get_mut(entity)
        {
            item.item = piece_item(piece);
        }
        if table.game.selected() == Some(step.to) {
            table.held = Some(entity);
        }
        table.landed.push(step);
    }
}

fn follow(table: &Table, visuals: &mut Visuals) {
    let Some(held) = table.held else {
        return;
    };
    let Some((x, z)) = table.aim.map(|(x, z)| geometry::clamp(x, z)) else {
        return;
    };
    let bob = BOB * (table.tick as f64 * 0.3).sin();
    if let Ok((mut position, _, _)) = visuals.get_mut(held) {
        set_position(&mut position, [x, REST_Y + HOLD_LIFT + bob, z]);
    }
}

/// The ghost shows where the piece would land: the aimed square when it is
/// legal, the square it came from otherwise.
fn draw_ghost(table: &Table, visuals: &mut Visuals) {
    let Some(ghost) = table.ghost else {
        return;
    };
    let Ok((mut position, mut display, mut item)) = visuals.get_mut(ghost) else {
        return;
    };
    let origin = table.held.and(table.game.selected());
    let scale = match origin {
        None => 0.0,
        Some(_) if (table.tick / GHOST_PULSE).is_multiple_of(2) => 1.0,
        Some(_) => 1.06,
    };
    if display.transform.scale[0] != scale {
        display.transform = DisplayTransform::default().uniform_scale(scale);
    }
    let Some(origin) = origin else {
        return;
    };
    let square = table
        .hover
        .filter(|square| table.game.targets().contains(square))
        .unwrap_or(origin);
    set_position(&mut position, rest(square));
    let king = table.game.piece(origin).is_some_and(|piece| piece.king);
    let wanted = ghost_item(king);
    if item.item != wanted {
        item.item = wanted;
    }
}

fn draw_hover(table: &mut Table, visuals: &mut Visuals, commands: &mut Commands) {
    let wanted = table
        .hover
        .filter(|_| table.held.is_none() && table.anims.is_empty())
        .filter(|square| table.game.can_grab(*square))
        .and_then(|square| table.token_at(square));
    if !table.anims.is_empty() {
        return;
    }
    for token in &table.tokens {
        if Some(token.entity) == table.held {
            continue;
        }
        let lifted = wanted.is_some_and(|w| w.entity == token.entity);
        let mut at = rest(token.square);
        at[1] += if lifted { HOVER_LIFT } else { 0.0 };
        if let Ok((mut position, _, _)) = visuals.get_mut(token.entity) {
            set_position(&mut position, at);
        }
    }
    let entity = wanted.map(|token| token.entity);
    if table.glowing != entity {
        if let Some(old) = table.glowing {
            commands.entity(old).try_remove::<Glowing>();
        }
        if let Some(new) = entity {
            commands.entity(new).insert(Glowing);
        }
        table.glowing = entity;
    }
}

fn tick(
    mut table: ResMut<Table>,
    rotations: Query<&Rotation>,
    mut visuals: Visuals,
    mut commands: Commands,
) {
    let table = &mut *table;
    table.tick += 1;
    if table.reset {
        table.reset = false;
        table.deal(&mut commands);
    }
    let Some(rotation) = table.player.and_then(|player| rotations.get(player).ok()) else {
        return;
    };
    table.aim = geometry::aim(rotation.yaw, rotation.pitch);
    table.hover = table.aim.and_then(|(x, z)| geometry::square_at(x, z));
    run_anims(table, &mut visuals, &mut commands);
    follow(table, &mut visuals);
    draw_ghost(table, &mut visuals);
    draw_hover(table, &mut visuals, &mut commands);
}

/// Separate from [`tick`]: these params read `Position`, which `tick` writes.
fn play_effects(
    mut table: ResMut<Table>,
    sounds: Sounds,
    particles: Particles,
    messages: Messages,
) {
    let Some(player) = table.player else {
        table.landed.clear();
        return;
    };
    for step in std::mem::take(&mut table.landed) {
        let at = rest(step.to);
        sounds.play_to(player, Sound::new("block.wood.place").pitch(0.9));
        if step.captured.is_some() {
            sounds.play_to(player, Sound::new("entity.item.break").pitch(1.2));
            particles
                .spawn(Particle::Poof)
                .at(at)
                .count(8)
                .speed(0.03)
                .send();
        }
        if step.promoted {
            sounds.play_to(player, Sound::new("entity.player.levelup").pitch(1.3));
            particles
                .spawn(Particle::TotemOfUndying)
                .at(at)
                .count(30)
                .speed(0.2)
                .send();
        }
    }
    if table.tick.is_multiple_of(STATUS_PERIOD) {
        messages.action_bar(player, table.status()).send();
    }
}

fn checkers_command() -> Command {
    CommandBuilder::new("dames")
        .description("Relance une partie de dames")
        .handler(restart)
        .build()
}

fn restart(ctx: &mut CommandContext) {
    ctx.with_world_mut(|world| world.resource_mut::<Table>().reset = true);
}

#[cfg(test)]
mod tests {
    use super::*;
    use voidmc::{
        CommandRegistry, DefaultWorldGenerator, RegistryDataStore, ServerConfig,
        ServerConfigResource, WorldGen,
        commands::plugin::CommandPlugin,
        network::{ClientConnected, IncomingPacket, NetworkPlugin, OutgoingPacket},
        plugins::DefaultPlugins,
        systems::GameSystemsPlugin,
        world::ChunkIndex,
    };

    /// Bevy only reports conflicting system params when the schedule first runs.
    #[test]
    fn the_plugin_ticks_with_the_full_engine() {
        let (_incoming_tx, incoming_rx) = flume::unbounded::<IncomingPacket>();
        let (outgoing_tx, _outgoing_rx) = flume::unbounded::<OutgoingPacket>();
        let (_disconnect_tx, disconnect_rx) = flume::unbounded::<u32>();
        let (kick_tx, _kick_rx) = flume::unbounded::<u32>();
        let (_connected_tx, connected_rx) = flume::unbounded::<ClientConnected>();

        let mut app = App::new();
        app.add_plugins(NetworkPlugin::new(
            incoming_rx,
            outgoing_tx,
            disconnect_rx,
            kick_tx,
            connected_rx,
        ))
        .add_plugins(DefaultPlugins)
        .add_plugins(CommandPlugin)
        .add_plugins(GameSystemsPlugin)
        .insert_resource(RegistryDataStore::default())
        .insert_resource(ServerConfigResource::from(&ServerConfig::default()))
        .insert_resource(WorldGen(Box::new(DefaultWorldGenerator::default())))
        .init_resource::<ChunkIndex>()
        .add_plugins(CheckersPlugin {
            pack_url: "http://localhost/pack.zip".into(),
        });
        assert!(app.world().contains_resource::<CommandRegistry>());

        app.update();
        app.update();
    }
}
