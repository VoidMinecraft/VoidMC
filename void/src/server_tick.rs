//! The number of the current tick, the one every development tool reads.

use std::sync::atomic::{AtomicU64, Ordering};

use bevy_app::{App, First, Plugin};

static TICK: AtomicU64 = AtomicU64::new(0);

/// How many updates have started: 1 during the first one, and 0 until [`install`] is called.
pub fn current() -> u64 {
    TICK.load(Ordering::Relaxed)
}

struct ServerTickPlugin;

impl Plugin for ServerTickPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(First, advance);
    }
}

fn advance() {
    TICK.fetch_add(1, Ordering::Relaxed);
}

/// Starts counting. A server without a tool that needs the number pays nothing.
pub fn install(app: &mut App) {
    if !app.is_plugin_added::<ServerTickPlugin>() {
        app.add_plugins(ServerTickPlugin);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_update_is_one_tick_however_often_it_is_installed() {
        let mut once = App::new();
        install(&mut once);
        let mut twice = App::new();
        install(&mut twice);
        install(&mut twice);
        let systems = |app: &App| app.get_schedule(First).map(|first| first.systems_len());
        assert_eq!(systems(&once), systems(&twice));

        let before = current();
        twice.update();
        twice.update();
        assert!(current() >= before + 2);
    }
}
