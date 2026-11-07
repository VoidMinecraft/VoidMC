mod configuration;
mod packs;
mod returns;

use uuid::Uuid;
use voidmc_codec::VarI32;
use voidmc_data::v26_1_2::packets::configuration::serverbound as config_sb;
use voidmc_data::v26_1_2::packets::login::serverbound as login_sb;
use voidmc_data::v26_1_2::packets::play::serverbound as play_sb;
use voidmc_protocol::clientbound::ResourcePackPush;

use super::*;

fn settings() -> Settings {
    Settings {
        return_timeout: Duration::from_secs(10),
        retry_every: Duration::from_millis(100),
        keep_alive_every: Duration::from_secs(1),
        settle: Duration::from_secs(3),
        backend_silence: Duration::from_secs(5),
    }
}

fn login_finished() -> Bytes {
    frame::encode(login_cb::LOGIN_FINISHED, |body| {
        Uuid::from_u128(77).encode(body);
    })
}

fn login() -> Bytes {
    frame::encode(play_cb::LOGIN, |body| body.push(7))
}

fn empty(id: i32) -> Bytes {
    frame::encode(id, |_| {})
}

fn hello() -> Bytes {
    frame::encode(login_sb::HELLO, |body| {
        "Bot".to_string().encode(body);
        Uuid::from_u128(1).encode(body);
    })
}

fn known_packs_question() -> Bytes {
    frame::encode(config_cb::SELECT_KNOWN_PACKS, |body| body.push(0))
}

fn known_packs_reply() -> Bytes {
    frame::encode(config_sb::SELECT_KNOWN_PACKS, |body| body.push(0))
}

fn registry(tag: u8) -> Bytes {
    frame::encode(config_cb::REGISTRY_DATA, |body| body.push(tag))
}

fn information() -> Bytes {
    frame::encode(config_sb::CLIENT_INFORMATION, |body| body.push(42))
}

fn push(id: Uuid, hash: &str) -> Bytes {
    let push = ResourcePackPush {
        id,
        url: "http://x".to_string(),
        hash: hash.to_string(),
        required: false,
        prompt: None,
    };
    frame::encode(play_cb::RESOURCE_PACK_PUSH, |body| push.encode(body))
}

fn config_push(id: Uuid) -> Bytes {
    let pushed = push(id, "abc");
    let (_, body) = frame::id_and_body(&pushed).unwrap();
    frame::encode(config_cb::RESOURCE_PACK_PUSH, |out| {
        out.extend_from_slice(body);
    })
}

fn pack_status(packet: i32, id: Uuid, status: i32) -> Bytes {
    frame::encode(packet, |body| {
        id.encode(body);
        VarI32(status).encode(body);
    })
}

fn pack_answer(id: Uuid, status: i32) -> Bytes {
    pack_status(play_sb::RESOURCE_PACK, id, status)
}

fn config_pack_answer(id: Uuid, status: i32) -> Bytes {
    pack_status(config_sb::RESOURCE_PACK, id, status)
}

fn count(frames: &[Bytes], id: i32) -> usize {
    frames
        .iter()
        .filter(|frame| frame::id_and_body(frame).unwrap().0 == id)
        .count()
}

struct Rig {
    session: Session,
    now: Instant,
    client: Vec<Bytes>,
    backend: Vec<Bytes>,
    rest: Vec<Action>,
}

impl Rig {
    fn fresh() -> Self {
        Self::with(settings())
    }

    fn with(settings: Settings) -> Self {
        let now = Instant::now();
        Self {
            session: Session::new(settings, now, empty(0)),
            now,
            client: Vec::new(),
            backend: Vec::new(),
            rest: Vec::new(),
        }
    }

    fn in_play() -> Self {
        let mut rig = Self::fresh();
        rig.join();
        rig
    }

    fn drain(&mut self) {
        for action in self.session.actions() {
            match action {
                Action::ToClient(frame) => self.client.push(frame),
                Action::ToBackend(frame) => self.backend.push(frame),
                other => self.rest.push(other),
            }
        }
    }

    fn clear(&mut self) {
        self.drain();
        self.client.clear();
        self.backend.clear();
        self.rest.clear();
    }

    fn tick(&mut self, by: Duration) {
        self.now += by;
        self.session.on_tick(self.now);
        self.drain();
    }

    fn receive_from_client(&mut self, frame: Bytes) {
        self.session.on_client(frame, self.now);
        self.drain();
    }

    fn receive_from_backend(&mut self, frame: Bytes) {
        self.session.on_backend(frame, self.now);
        self.drain();
    }

    fn lose_backend(&mut self) {
        self.session.on_backend_lost(self.now);
        self.drain();
    }

    fn connected(&mut self) {
        self.session.on_backend_connected(self.now);
        self.drain();
    }

    /// A first join, up to the registries of the configuration.
    fn reach_configuration(&mut self) {
        self.tick(Duration::ZERO);
        self.connected();
        self.receive_from_client(hello());
        self.receive_from_backend(login_finished());
        self.receive_from_client(empty(login_sb::LOGIN_ACKNOWLEDGED));
        self.receive_from_client(information());
        self.receive_from_backend(known_packs_question());
        self.receive_from_client(known_packs_reply());
        self.receive_from_backend(registry(1));
    }

    fn join(&mut self) {
        self.reach_configuration();
        self.receive_from_backend(empty(config_cb::FINISH_CONFIGURATION));
        self.receive_from_client(empty(config_sb::FINISH_CONFIGURATION));
        self.receive_from_backend(login());
        self.receive_from_client(empty(play_sb::PLAYER_LOADED));
        self.clear();
    }

    fn in_configuration() -> Self {
        let mut rig = Self::fresh();
        rig.reach_configuration();
        rig.clear();
        rig
    }

    fn log_back_in(&mut self) {
        self.tick(settings().retry_every);
        self.connected();
        self.receive_from_backend(login_finished());
    }

    /// A client in play taken through the configuration of a new server, up to its `login`.
    fn come_back(&mut self) {
        self.log_back_in();
        self.receive_from_client(empty(play_sb::CONFIGURATION_ACKNOWLEDGED));
        self.receive_from_backend(known_packs_question());
        self.receive_from_client(known_packs_reply());
        self.receive_from_backend(registry(1));
        self.receive_from_backend(empty(config_cb::FINISH_CONFIGURATION));
        self.receive_from_client(empty(config_sb::FINISH_CONFIGURATION));
        self.receive_from_backend(login());
    }

    fn events(&self) -> Vec<&Event> {
        self.rest
            .iter()
            .filter_map(|action| match action {
                Action::Event(event) => Some(event),
                _ => None,
            })
            .collect()
    }

    fn resumed(&self) -> usize {
        self.events()
            .into_iter()
            .filter(|event| matches!(event, Event::Resumed { .. }))
            .count()
    }

    fn sent_to_client(&self, id: i32) -> usize {
        count(&self.client, id)
    }

    fn sent_to_backend(&self, id: i32) -> usize {
        count(&self.backend, id)
    }
}
