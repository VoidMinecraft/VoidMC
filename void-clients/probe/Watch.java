import java.util.function.BooleanSupplier;
import java.util.function.Consumer;

final class Watch {
  private final Retry retry;
  private final Consumer<Throwable> report;
  private final Guard guard = new Guard();

  Watch(Retry retry, Consumer<Throwable> report) {
    this.retry = retry;
    this.report = report;
  }

  void look(long now, boolean inGame, Object screen, BooleanSupplier portOpen, Consumer<Runnable> execute, Runnable reconnect) {
    Throwable fresh = guard.run(() -> {
      boolean disconnected = screen != null && screen.getClass().getSimpleName().equals("DisconnectedScreen");
      if (retry.look(now, inGame, disconnected ? screen : null, portOpen)) execute.accept(retry.task(reconnect));
    });
    if (fresh == null) return;
    try {
      report.accept(fresh);
    } catch (Throwable unreported) {
      return;
    }
  }
}
