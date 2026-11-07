import com.mojang.blaze3d.platform.FramerateLimitTracker;
import com.mojang.blaze3d.systems.TimerQuery;
import java.io.File;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Comparator;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;
import java.util.concurrent.Callable;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.TimeUnit;
import jdk.jfr.consumer.RecordedEvent;
import jdk.jfr.consumer.RecordingFile;
import net.minecraft.client.CloudStatus;
import net.minecraft.client.Minecraft;
import net.minecraft.client.OptionInstance;
import net.minecraft.client.particle.Particle;
import net.minecraft.core.particles.ParticleTypes;
import net.minecraft.util.profiling.jfr.Environment;
import net.minecraft.util.profiling.jfr.JvmProfiler;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.phys.Vec3;
import org.joml.Vector3fc;

/** The cost of a frame: F3+L, whole frames on the GPU, and JFR, each started while in game. */
final class Perf {
  private static final long SETTLE_MS = 20_000;
  private static final long STILL_MS = 1_500;
  private static final int FRAMES_AFTER_A_CHANGE = 4;
  private static final long METRICS_LIMIT_S = 10;
  private static final int PARTICLE_LIFE = 1_000_000;
  private static final int SAMPLE_FRAMES = 8;
  private static final long NO_SAMPLE = -1;
  private static final int UNLIMITED_FPS = 260;

  private Perf() {}

  static Map<String, Object> run(Minecraft mc, String[] w) throws Exception {
    if (Probe.isLogic() && !w[1].equals("metrics") && !w[1].equals("jfr")) {
      throw new IllegalStateException("the logic level has no GPU to time");
    }
    return switch (w[1]) {
      case "gpu" -> gpu(mc, Integer.parseInt(w[2]));
      case "ab" -> ab(mc, w[2], Integer.parseInt(w[3]), Integer.parseInt(w[4]), w.length > 5 ? Integer.parseInt(w[5]) : 0);
      case "metrics" -> metrics(mc, Long.parseLong(w[2]));
      case "jfr" -> jfr(Long.parseLong(w[2]));
      default -> throw new IllegalArgumentException("perf gpu, ab, metrics or jfr");
    };
  }

  private static <T> T onGame(Minecraft mc, Callable<T> task) throws Exception {
    CompletableFuture<T> result = new CompletableFuture<>();
    mc.execute(() -> {
      try {
        result.complete(task.call());
      } catch (Throwable error) {
        result.completeExceptionally(error);
      }
    });
    return result.get(10, TimeUnit.SECONDS);
  }

  private static int frameCount(Minecraft mc) throws Exception {
    Field frames = Minecraft.class.getDeclaredField("frames");
    frames.setAccessible(true);
    return frames.getInt(mc);
  }

  /** The game sets this counter back to 0 every second, so only a change means a frame. */
  private static void waitFrames(Minecraft mc, int count) throws Exception {
    long limit = System.currentTimeMillis() + 10_000;
    int last = frameCount(mc);
    for (int seen = 0; seen < count; ) {
      if (System.currentTimeMillis() > limit) throw new IllegalStateException("no frame for 10 s");
      int now = frameCount(mc);
      if (now != last) {
        last = now;
        seen++;
      } else {
        Thread.sleep(1);
      }
    }
  }

  private record Scene(int sections, boolean complete, int queue, double[] camera, Map<Integer, Vec3> entities) {
    boolean sameAs(Scene other) {
      return sections == other.sections && Arrays.equals(camera, other.camera);
    }

    int movedSince(Scene other) {
      int moved = 0;
      for (Map.Entry<Integer, Vec3> entity : entities.entrySet()) {
        Vec3 before = other.entities.get(entity.getKey());
        if (before == null || before.distanceToSqr(entity.getValue()) > 1e-6) moved++;
      }
      return moved;
    }
  }

  private static Scene scene(Minecraft mc) throws Exception {
    return onGame(mc, () -> {
      var renderer = mc.levelRenderer;
      var camera = mc.gameRenderer.getMainCamera();
      Vec3 at = camera.position();
      Map<Integer, Vec3> entities = new HashMap<>();
      for (Entity entity : mc.level.entitiesForRendering()) entities.put(entity.getId(), entity.position());
      return new Scene(renderer.countRenderedSections(), renderer.hasRenderedAllSections(),
        renderer.getSectionRenderDispatcher().getCompileQueueSize(),
        new double[] {at.x, at.y, at.z, camera.yRot(), camera.xRot()}, entities);
    });
  }

  /** Waits for the sections to stop changing, or a measure would count their upload. */
  private static boolean settle(Minecraft mc) throws Exception {
    long limit = System.currentTimeMillis() + SETTLE_MS;
    long since = System.currentTimeMillis();
    Scene last = scene(mc);
    while (System.currentTimeMillis() < limit) {
      Thread.sleep(100);
      Scene now = scene(mc);
      if (!now.sameAs(last) || now.queue > 0 || !now.complete || mc.getOverlay() != null) since = System.currentTimeMillis();
      else if (System.currentTimeMillis() - since >= STILL_MS) return true;
      last = now;
    }
    return false;
  }

  /**
   * Nanoseconds per frame, each sample the mean of a few frames. The frame limiter stays out of
   * it, or a sample is only the length of the wait: at 40 images per second, 25 ms. A sample
   * across the second in which the game resets its counter is dropped.
   */
  private static List<Long> timed(Minecraft mc, int frames) throws Exception {
    TimerQuery query = TimerQuery.getInstance();
    record Open(TimerQuery.FrameProfile profile, int frames) {}
    List<Open> open = new ArrayList<>();
    for (int i = 0; i < Math.max(1, frames / SAMPLE_FRAMES); i++) {
      int begun = onGame(mc, () -> {
        if (query.isRecording()) return -1;
        query.beginProfile();
        return frameCount(mc);
      });
      if (begun < 0) {
        waitFrames(mc, 1);
        continue;
      }
      try {
        waitFrames(mc, SAMPLE_FRAMES);
      } finally {
        onGame(mc, () -> {
          TimerQuery.FrameProfile profile = query.endProfile();
          int spanned = frameCount(mc) - begun;
          if (spanned > 0) open.add(new Open(profile, spanned));
          else profile.cancel();
          return 0;
        });
      }
    }
    List<Long> times = new ArrayList<>();
    long limit = System.currentTimeMillis() + 3_000;
    while (!open.isEmpty() && System.currentTimeMillis() < limit) {
      onGame(mc, () -> {
        for (var left = open.iterator(); left.hasNext(); ) {
          Open sample = left.next();
          if (sample.profile.isDone()) {
            times.add(sample.profile.get() / sample.frames);
            left.remove();
          }
        }
        return 0;
      });
      Thread.sleep(10);
    }
    onGame(mc, () -> {
      open.forEach(sample -> sample.profile.cancel());
      return 0;
    });
    return times;
  }

  private record Window(List<Long> times, boolean still, int moved) {}

  /** What the limit is now: `fps` sets it past the options, which know nothing of that. */
  private static int frameLimit(FramerateLimitTracker limiter) throws Exception {
    Field limit = FramerateLimitTracker.class.getDeclaredField("framerateLimit");
    limit.setAccessible(true);
    return limit.getInt(limiter);
  }

  private interface Frozen<T> {
    T run() throws Exception;
  }

  /** Stops the entities on this client during a task, whatever the server sends. */
  private static <T> T frozen(Minecraft mc, Frozen<T> task) throws Exception {
    var ticks = mc.level.tickRateManager();
    boolean was = onGame(mc, () -> {
      boolean frozen = ticks.isFrozen();
      ticks.setFrozen(true);
      return frozen;
    });
    try {
      waitFrames(mc, FRAMES_AFTER_A_CHANGE);
      return task.run();
    } finally {
      onGame(mc, () -> {
        ticks.setFrozen(was);
        return 0;
      });
    }
  }

  private static Window window(Minecraft mc, int frames) throws Exception {
    var limiter = mc.getFramerateLimitTracker();
    int fps = onGame(mc, () -> {
      int limit = frameLimit(limiter);
      limiter.setFramerateLimit(UNLIMITED_FPS);
      return limit;
    });
    try {
      return frozen(mc, () -> {
        Scene before = scene(mc);
        List<Long> times = timed(mc, frames);
        Scene after = scene(mc);
        return new Window(times, after.sameAs(before), after.movedSince(before));
      });
    } finally {
      onGame(mc, () -> {
        limiter.setFramerateLimit(fps);
        return 0;
      });
    }
  }

  /** -1 when a window has no sample: voidc refuses a comparison that has one. */
  private static long median(List<Long> values) {
    if (values.isEmpty()) return NO_SAMPLE;
    List<Long> sorted = new ArrayList<>(values);
    sorted.sort(Comparator.naturalOrder());
    return sorted.get(sorted.size() / 2);
  }

  private static Map<String, Object> gpu(Minecraft mc, int frames) throws Exception {
    boolean settled = settle(mc);
    Window window = window(mc, frames);
    Map<String, Object> reply = new LinkedHashMap<>();
    reply.put("settled", settled);
    reply.put("still", window.still);
    reply.put("moved_entities", window.moved);
    reply.put("sections", scene(mc).sections);
    reply.put("ns", new ArrayList<Object>(window.times));
    return reply;
  }

  private interface Side {
    void apply() throws Exception;
  }

  private static void put(Minecraft mc, OptionInstance<?> option, Object value) throws Exception {
    Field field = OptionInstance.class.getDeclaredField("value");
    field.setAccessible(true);
    onGame(mc, () -> {
      field.set(option, value);
      return 0;
    });
  }

  /**
   * Particles die and nothing on a map makes any, so the second side makes its own: many, far
   * from dying, in front of the camera. What it measures is the price of that many.
   */
  private static void spawn(Minecraft mc, int count) throws Exception {
    onGame(mc, () -> {
      mc.particleEngine.clearParticles();
      var camera = mc.gameRenderer.getMainCamera();
      Vec3 at = camera.position();
      Vector3fc forward = camera.forwardVector();
      int side = (int) Math.ceil(Math.cbrt(count));
      for (int i = 0; i < count; i++) {
        double x = at.x + forward.x() * 12 + (i % side - side / 2.0) * 0.5;
        double y = at.y + forward.y() * 12 + (i / side % side - side / 2.0) * 0.5;
        double z = at.z + forward.z() * 12 + (i / side / side - side / 2.0) * 0.5;
        Particle particle = mc.particleEngine.createParticle(ParticleTypes.END_ROD, x, y, z, 0, 0, 0);
        if (particle != null) particle.setLifetime(PARTICLE_LIFE);
      }
      return 0;
    });
  }

  private static int particles(Minecraft mc) throws Exception {
    return onGame(mc, () -> Integer.parseInt(mc.particleEngine.countParticles()));
  }

  private static void clear(Minecraft mc) throws Exception {
    onGame(mc, () -> {
      mc.particleEngine.clearParticles();
      return 0;
    });
  }

  /**
   * Compares two settings in turns, A B then B A, so a drift of the machine falls on both.
   * Clouds are read at each frame, so nothing has to be reloaded: `allChanged` is what rebuilt
   * the chunks during the measures of the study. The option is written directly because its
   * setter would also turn the graphics preset to custom.
   */
  private static Map<String, Object> ab(Minecraft mc, String what, int rounds, int frames, int count) throws Exception {
    Side first, second, restore;
    String labelA, labelB;
    switch (what) {
      case "clouds" -> {
        OptionInstance<CloudStatus> option = mc.options.cloudStatus();
        CloudStatus was = onGame(mc, option::get);
        CloudStatus on = was == CloudStatus.OFF ? CloudStatus.FANCY : was;
        first = () -> put(mc, option, on);
        second = () -> put(mc, option, CloudStatus.OFF);
        restore = () -> put(mc, option, was);
        labelA = "clouds " + on;
        labelB = "clouds off";
      }
      case "particles" -> {
        int wanted = count > 0 ? count : 2000;
        first = () -> clear(mc);
        second = () -> {
          spawn(mc, wanted);
          long limit = System.currentTimeMillis() + 2_000;
          while (particles(mc) < wanted && System.currentTimeMillis() < limit) Thread.sleep(10);
        };
        restore = () -> clear(mc);
        labelA = "no particles";
        labelB = wanted + " particles";
      }
      default -> throw new IllegalArgumentException("perf ab clouds or particles");
    }
    boolean settled = settle(mc);
    Scene before = scene(mc);
    List<Object> a = new ArrayList<>(), b = new ArrayList<>(), shots = new ArrayList<>();
    int reloads = 0, seen = 0, moved = 0;
    boolean still = true;
    try {
      for (int round = 0; round < rounds; round++) {
        for (int turn = 0; turn < 2; turn++) {
          boolean isA = (turn == 0) == (round % 2 == 0);
          (isA ? first : second).apply();
          waitFrames(mc, FRAMES_AFTER_A_CHANGE);
          Window window = window(mc, frames);
          still &= window.still;
          moved = Math.max(moved, window.moved);
          Scene now = scene(mc);
          if (now.queue > 0 || now.sections != before.sections) reloads++;
          if (!isA && what.equals("particles")) seen = Math.max(seen, particles(mc));
          (isA ? a : b).add(median(window.times));
        }
      }
      first.apply();
      shots.add(frozen(mc, () -> Probe.shot("perf-a", 1)));
      second.apply();
      shots.add(frozen(mc, () -> Probe.shot("perf-b", 1)));
    } finally {
      restore.apply();
    }
    Map<String, Object> reply = new LinkedHashMap<>();
    reply.put("shots", shots);
    reply.put("a", labelA);
    reply.put("b", labelB);
    reply.put("a_ns", a);
    reply.put("b_ns", b);
    reply.put("settled", settled);
    reply.put("reloads", reloads);
    reply.put("still", still);
    reply.put("moved_entities", moved);
    if (what.equals("particles")) reply.put("particles", seen);
    return reply;
  }

  /** F3+L stops by itself after ten seconds, and only then writes its archive. */
  private static Map<String, Object> metrics(Minecraft mc, long seconds) throws Exception {
    File debug = new File("debug/profiling").getAbsoluteFile();
    File[] old = debug.listFiles();
    List<String> known = new ArrayList<>();
    if (old != null) for (File file : old) known.add(file.getName());
    CompletableFuture<Object> done = new CompletableFuture<>();
    boolean started = onGame(mc, () -> mc.debugClientMetricsStart(done::complete));
    if (!started) throw new IllegalStateException("a recording is already running");
    long wanted = Math.min(seconds, METRICS_LIMIT_S);
    Thread.sleep(wanted * 1000);
    Method stop = Minecraft.class.getDeclaredMethod("debugClientMetricsStop");
    stop.setAccessible(true);
    onGame(mc, () -> stop.invoke(mc));
    done.get(20, TimeUnit.SECONDS);
    long limit = System.currentTimeMillis() + 10_000;
    while (System.currentTimeMillis() < limit) {
      File[] now = debug.listFiles();
      if (now != null) {
        for (File file : now) {
          if (file.getName().endsWith(".zip") && !known.contains(file.getName()) && file.length() > 0) {
            Map<String, Object> reply = new LinkedHashMap<>();
            reply.put("path", file.getPath());
            reply.put("seconds", wanted);
            return reply;
          }
        }
      }
      Thread.sleep(100);
    }
    throw new IllegalStateException("no archive in " + debug);
  }

  private static Map<String, Object> jfr(long seconds) throws Exception {
    if (!JvmProfiler.INSTANCE.start(Environment.CLIENT)) throw new IllegalStateException("JFR did not start");
    Thread.sleep(seconds * 1000);
    Path file = JvmProfiler.INSTANCE.stop();
    Map<String, long[]> received = new TreeMap<>(), sent = new TreeMap<>();
    try (RecordingFile recording = new RecordingFile(file)) {
      while (recording.hasMoreEvents()) {
        RecordedEvent event = recording.readEvent();
        String name = event.getEventType().getName();
        Map<String, long[]> side = name.equals("minecraft.PacketReceived") ? received
          : name.equals("minecraft.PacketSent") ? sent : null;
        if (side == null) continue;
        long[] row = side.computeIfAbsent(event.getString("packetId"), key -> new long[2]);
        row[0]++;
        row[1] += event.getInt("bytes");
      }
    }
    Map<String, Object> reply = new LinkedHashMap<>();
    reply.put("path", file.toAbsolutePath().toString());
    reply.put("seconds", seconds);
    reply.put("received", rows(received));
    reply.put("sent", rows(sent));
    return reply;
  }

  private static List<Object> rows(Map<String, long[]> packets) {
    List<Object> rows = new ArrayList<>();
    packets.forEach((name, row) -> rows.add(List.of(name, row[0], row[1])));
    return rows;
  }
}
