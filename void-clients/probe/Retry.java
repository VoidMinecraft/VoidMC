import java.util.function.BooleanSupplier;

final class Retry {
  private static final long STEADY_MS = 1_000;
  private static final long STALE_MS = 5_000;
  private final long everyMs;
  private final long longestMs;
  private long last = Long.MIN_VALUE / 2;
  private long queuedAt = Long.MIN_VALUE / 2;
  private boolean queued;
  private int kicks;
  private boolean playing;
  private long playingSince;
  private boolean steady;
  private boolean serverAway;
  private Object away;

  Retry(long everyMs, long longestMs) {
    this.everyMs = everyMs;
    this.longestMs = longestMs;
  }

  synchronized boolean look(long now, boolean inGame, Object disconnected, BooleanSupplier portOpen) {
    if (inGame && !playing) {
      playing = true;
      playingSince = now;
    }
    if (!inGame && playing) {
      playing = false;
      steady = now - playingSince >= STEADY_MS;
    }
    if (disconnected == null) return false;
    if (disconnected != away) {
      away = disconnected;
      serverAway = !portOpen.getAsBoolean();
      kicks = steady || serverAway ? 0 : kicks + 1;
      steady = false;
    }
    if (queued && now - queuedAt < STALE_MS) return false;
    if (serverAway) {
      if (!portOpen.getAsBoolean()) return false;
      serverAway = false;
    } else if (now - last < Math.min(longestMs, everyMs << Math.min(kicks, 8))) {
      return false;
    }
    last = now;
    queuedAt = now;
    queued = true;
    return true;
  }

  Runnable task(Runnable attempt) {
    return () -> {
      synchronized (this) {
        queued = false;
      }
      attempt.run();
    };
  }
}
