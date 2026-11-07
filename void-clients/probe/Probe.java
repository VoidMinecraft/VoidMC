import com.mojang.blaze3d.platform.Window;
import java.io.BufferedReader;
import java.io.File;
import java.io.FileWriter;
import java.io.IOException;
import java.io.InputStreamReader;
import java.io.PrintWriter;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.net.StandardProtocolFamily;
import java.net.UnixDomainSocketAddress;
import java.nio.channels.Channels;
import java.nio.channels.ServerSocketChannel;
import java.nio.channels.SocketChannel;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.concurrent.Callable;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.Executor;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.TimeoutException;
import java.util.concurrent.atomic.AtomicLong;
import java.util.concurrent.locks.LockSupport;
import java.util.function.Supplier;
import io.netty.channel.Channel;
import io.netty.channel.ChannelHandlerContext;
import io.netty.channel.ChannelInboundHandlerAdapter;
import io.netty.channel.ChannelOutboundHandlerAdapter;
import io.netty.channel.ChannelPipeline;
import io.netty.channel.ChannelPromise;
import io.netty.handler.timeout.ReadTimeoutHandler;
import net.minecraft.client.DeltaTracker;
import net.minecraft.client.Minecraft;
import net.minecraft.client.Screenshot;
import net.minecraft.client.gui.components.ChatComponent;
import net.minecraft.client.gui.screens.ChatScreen;
import net.minecraft.client.gui.screens.ConnectScreen;
import net.minecraft.client.gui.screens.Screen;
import net.minecraft.client.gui.screens.TitleScreen;
import net.minecraft.client.input.KeyEvent;
import net.minecraft.client.input.MouseButtonEvent;
import net.minecraft.client.input.MouseButtonInfo;
import net.minecraft.client.multiplayer.ServerData;
import net.minecraft.client.multiplayer.resolver.ServerAddress;
import net.minecraft.client.player.LocalPlayer;
import net.minecraft.network.Connection;
import net.minecraft.network.chat.Component;
import net.minecraft.network.chat.contents.TranslatableContents;
import net.minecraft.util.TimeSource;
import net.minecraft.util.Util;
import net.minecraft.world.entity.Entity;
import org.lwjgl.glfw.GLFW;
import org.lwjgl.system.JNI;
import org.lwjgl.system.macosx.ObjCRuntime;

/** Runs the game and answers verbs on a unix socket, one line in, one JSON line out. */
public class Probe {
  private static final String LEVEL = System.getProperty("void.level", "render");
  private static final String SERVER = System.getProperty("void.server");
  private static final String VERSION = System.getProperty("void.version", "");
  private static final long IDLE_MS = Long.getLong("void.idle.minutes", 30) * 60_000;
  private static volatile long lastVerb = System.currentTimeMillis();
  private static volatile boolean shown;
  private static volatile int reconnects;

  public static void main(String[] args) throws Exception {
    keepTitle(System.getProperty("void.title"));
    if (!LEVEL.equals("play")) {
      daemon("void-verbs", Probe::serve);
      daemon("void-watch", Probe::watch);
    }
    net.minecraft.client.main.Main.main(args);
  }

  static final Map<String, GivenBack> RECORDING = new ConcurrentHashMap<>();
  static final String SPOILED = "invalid: the clock went back to real time during this recording";

  private interface Task {
    void run() throws Exception;
  }

  interface Writing {
    void run(Recorder out) throws Exception;
  }

  static class GivenBack {
    private volatile boolean set;

    void set() {
      set = true;
    }

    boolean isSet() {
      return set;
    }
  }

  /** Only on the game thread, where `release` runs: a line stamped after it finds `givenBack` set. */
  static final class Recorder {
    private final PrintWriter out;
    private final GivenBack givenBack;
    private volatile boolean late;

    Recorder(PrintWriter out, GivenBack givenBack) {
      this.out = out;
      this.givenBack = givenBack;
    }

    void stamp(String line) {
      out.println(line);
      if (givenBack.isSet()) late = true;
    }
  }

  private static void daemon(String name, Task task) {
    Thread thread = new Thread(() -> {
      try {
        task.run();
      } catch (Throwable error) {
        error.printStackTrace();
      }
    }, name);
    thread.setDaemon(true);
    thread.start();
  }

  private static void keepTitle(String title) {
    daemon("void-title", () -> {
      while (true) {
        Thread.sleep(1000);
        Minecraft mc = Minecraft.getInstance();
        if (mc != null && mc.getWindow() != null) mc.execute(() -> mc.getWindow().setTitle(title));
      }
    });
  }

  private static void watch() throws Exception {
    while (true) {
      Thread.sleep(2000);
      Minecraft mc = Minecraft.getInstance();
      if (mc == null) continue;
      if (!shown && System.currentTimeMillis() - lastVerb > IDLE_MS) {
        mc.execute(mc::stop);
        return;
      }
      if (!Stepping.virtual() && mc.screen != null && mc.screen.getClass().getSimpleName().equals("DisconnectedScreen")) {
        mc.execute(() -> reconnect(mc));
      }
    }
  }

  private static void reconnect(Minecraft mc) {
    ServerData data = new ServerData("void", SERVER, ServerData.Type.OTHER);
    data.setResourcePackStatus(ServerData.ServerPackStatus.ENABLED);
    reconnects++;
    ConnectScreen.startConnecting(new TitleScreen(), mc, ServerAddress.parseString(SERVER), data, false, null);
  }

  private static void serve() throws Exception {
    Path socket = Path.of(System.getProperty("void.socket"));
    Files.deleteIfExists(socket);
    ServerSocketChannel server = ServerSocketChannel.open(StandardProtocolFamily.UNIX);
    server.bind(UnixDomainSocketAddress.of(socket));
    while (true) {
      SocketChannel channel = server.accept();
      daemon("void-connection", () -> connection(channel));
    }
  }

  private static void connection(SocketChannel channel) throws IOException {
    try (channel;
        BufferedReader in = new BufferedReader(new InputStreamReader(Channels.newInputStream(channel), StandardCharsets.UTF_8));
        PrintWriter out = new PrintWriter(Channels.newOutputStream(channel), true, StandardCharsets.UTF_8)) {
      String line;
      while ((line = in.readLine()) != null) out.println(answer(line));
    }
  }

  private static final Map<String, String> USAGE = Map.ofEntries(
      Map.entry("shot", "shot name [downscale]"),
      Map.entry("fps", "fps n"),
      Map.entry("turn", "turn dx dy"),
      Map.entry("aim", "aim yaw pitch"),
      Map.entry("seq", "seq ms dx,dy ..."),
      Map.entry("shift", "shift on|off"),
      Map.entry("key", "key code action"),
      Map.entry("hover", "hover x y"),
      Map.entry("click", "click x y"),
      Map.entry("track", "track n"),
      Map.entry("frames", "frames n"));

  private static int required(String usage) {
    return (int) Arrays.stream(usage.split(" ")).skip(1).filter(word -> !word.startsWith("[") && !word.equals("...")).count();
  }

  private static String answer(String line) {
    String[] parts = Condition.split(line);
    String name = parts[0], rest = parts[1];
    List<String> all = new ArrayList<>(List.of(name));
    if (!rest.isEmpty()) all.addAll(List.of(rest.split("[ \t]+")));
    String[] words = all.toArray(String[]::new);
    String usage = USAGE.get(name);
    if (usage != null && words.length - 1 < required(usage)) return failure("usage: " + usage);
    try {
      if (!name.equals("state") && !name.equals("thumb")) lastVerb = System.currentTimeMillis();
      return ok(verb(name, words, rest));
    } catch (Throwable thrown) {
      Throwable error = thrown instanceof java.util.concurrent.ExecutionException ? thrown.getCause() : thrown;
      boolean malformed = error instanceof NumberFormatException || error instanceof ArrayIndexOutOfBoundsException;
      if (usage != null && malformed) return failure("usage: " + usage + " (" + error.getMessage() + ")");
      return failure(String.valueOf(error));
    }
  }

  private static <T> T onGame(Callable<T> task) throws Exception {
    return onGame(task, 10_000);
  }

  private static <T> T onGame(Callable<T> task, long withinMs) throws Exception {
    Minecraft mc = ready();
    CompletableFuture<T> result = new CompletableFuture<>();
    mc.execute(() -> {
      try {
        result.complete(task.call());
      } catch (Throwable error) {
        result.completeExceptionally(error);
      }
    });
    return result.get(withinMs, TimeUnit.MILLISECONDS);
  }

  private static Minecraft ready() {
    Minecraft mc = Minecraft.getInstance();
    if (mc == null) throw new IllegalStateException("game not ready");
    return mc;
  }

  private static LocalPlayer player() {
    LocalPlayer player = ready().player;
    if (player == null) throw new IllegalStateException("not in a world");
    return player;
  }

  private static Map<String, Object> verb(String verb, String[] w, String rest) throws Exception {
    if (verb.equals("wait_until")) return waitUntil(rest);
    Minecraft mc = ready();
    Map<String, Object> reply = new LinkedHashMap<>();
    switch (verb) {
      case "state" -> reply.putAll(onGame(Probe::fields));
      case "act" -> onGame(() -> { player().connection.sendCommand(rest); return 0; });
      case "assert" -> {
        Map<String, Object> fields = onGame(Probe::fields);
        Condition.Parsed parsed = Condition.parse(rest);
        Condition.validate(parsed, LEVEL);
        if (!Condition.holds(parsed, fields)) throw new AssertionError(rest + " is false: " + fields);
        reply.putAll(fields);
      }
      case "shot" -> reply.put("path", shot(w[1], w.length > 2 ? Integer.parseInt(w[2]) : 1));
      case "thumb" -> reply.put("path", shot("thumb", 2));
      case "fps" -> onGame(() -> { mc.getFramerateLimitTracker().setFramerateLimit(Integer.parseInt(w[1])); return 0; });
      case "show" -> onGame(() -> { show(mc, w.length > 1 && w[1].equals("focus")); return 0; });
      case "hide" -> onGame(() -> { hide(mc); return 0; });
      case "turn" -> onGame(() -> { player().turn(Double.parseDouble(w[1]), Double.parseDouble(w[2])); return 0; });
      case "aim" -> onGame(() -> { player().setYRot(Float.parseFloat(w[1])); player().setXRot(Float.parseFloat(w[2])); return 0; });
      case "seq" -> seq(w);
      case "attack" -> reply.put("hit", onGame(() -> invoke(mc, "startAttack")));
      case "use" -> onGame(() -> invoke(mc, "startUseItem"));
      case "shift" -> onGame(() -> { mc.options.keyShift.setDown(w[1].equals("on")); return 0; });
      case "key" -> onGame(() -> { key(mc, Integer.parseInt(w[1]), Integer.parseInt(w[2])); return 0; });
      case "hover" -> onGame(() -> { hover(mc, Double.parseDouble(w[1]), Double.parseDouble(w[2])); return 0; });
      case "click" -> reply.put("handled", onGame(() -> click(mc, Double.parseDouble(w[1]), Double.parseDouble(w[2]))));
      case "free" -> onGame(() -> { mc.mouseHandler.releaseMouse(); return 0; });
      case "type" -> onGame(() -> { mc.setScreen(new ChatScreen(rest, false)); return 0; });
      case "esc" -> onGame(() -> { screen(mc).onClose(); return 0; });
      case "dump" -> reply.putAll(onGame(() -> dump(mc)));
      case "entities" -> reply.put("entities", onGame(() -> entities(mc)));
      case "track" -> track(mc, Integer.parseInt(w[1]));
      case "frames" -> frames(mc, Integer.parseInt(w[1]));
      case "clock" -> reply.put("clock", Stepping.clock(mc, w[1]));
      case "step" -> reply.putAll(Stepping.step(mc, Double.parseDouble(w[1]), w.length > 2 ? Integer.parseInt(w[2]) : 1));
      case "fence" -> reply.put("in", Stepping.fence(Long.parseLong(w[1])));
      case "sent" -> reply.putAll(Stepping.counters());
      case "perf" -> reply.putAll(Perf.run(mc, w));
      case "quit" -> mc.execute(mc::stop);
      default -> throw new IllegalArgumentException("unknown verb " + verb);
    }
    return reply;
  }

  static boolean isLogic() {
    return LEVEL.equals("logic");
  }

  private static Screen screen(Minecraft mc) {
    if (mc.screen == null) throw new IllegalStateException("no screen open");
    return mc.screen;
  }

  private static Object invoke(Object target, String name) throws Exception {
    Method method = target.getClass().getDeclaredMethod(name);
    method.setAccessible(true);
    Object result = method.invoke(target);
    return result == null ? 0 : result;
  }

  private static Map<String, Object> fields() throws Exception {
    Minecraft mc = ready();
    Map<String, Object> fields = new LinkedHashMap<>();
    String screen = mc.screen == null ? "" : mc.screen.getClass().getSimpleName();
    String state = screen.equals("DisconnectedScreen") ? "disconnected"
      : mc.level != null ? "connected"
      : screen.contains("Connect") || screen.contains("Progress") || screen.contains("Reconfig") ? "connecting" : "waiting";
    fields.put("state", state);
    fields.put("screen", screen);
    fields.put("loading", mc.getOverlay() != null);
    fields.put("fps", mc.getFps());
    fields.put("level", LEVEL);
    fields.put("version", VERSION);
    fields.put("reconnects", reconnects);
    fields.put("shown", shown);
    fields.put("clock", Stepping.virtual() ? "virtual" : "real");
    LocalPlayer player = mc.player;
    if (player != null) {
      fields.put("x", player.getX());
      fields.put("y", player.getY());
      fields.put("z", player.getZ());
      fields.put("yaw", player.getYRot());
      fields.put("pitch", player.getXRot());
      fields.put("passenger", player.isPassenger());
      fields.put("spectator", player.isSpectator());
    }
    if (mc.level != null) {
      fields.put("dimension", mc.level.dimension().identifier().toString());
      fields.put("chunks", mc.level.getChunkSource().getLoadedChunksCount());
      fields.put("entities", mc.level.getEntityCount());
      Entity camera = mc.getCameraEntity();
      fields.put("camera", camera == null ? "" : camera.getType().toShortString());
      var view = mc.gameRenderer.getMainCamera();
      fields.put("cam_x", view.position().x);
      fields.put("cam_y", view.position().y);
      fields.put("cam_z", view.position().z);
      fields.put("cam_yaw", view.yRot());
      fields.put("cam_pitch", view.xRot());
    }
    fields.put("chat", lastChat(mc));
    return fields;
  }

  private static String lastChat(Minecraft mc) throws Exception {
    Field field = ChatComponent.class.getDeclaredField("allMessages");
    field.setAccessible(true);
    List<?> messages = (List<?>) field.get(mc.gui.getChat());
    if (messages.isEmpty()) return "";
    Object newest = messages.get(0);
    Object content = newest.getClass().getMethod("content").invoke(newest);
    return ((net.minecraft.network.chat.Component) content).getString();
  }

  private static Map<String, Object> waitUntil(String rest) throws Exception {
    Condition.Wait wait = Condition.waitOf(rest);
    Condition.validate(wait.parsed(), LEVEL);
    return Condition.await(wait, rest, within -> onGame(Probe::fields, Math.min(10_000, within)), 50);
  }

  static String shot(String name, int downscale) throws Exception {
    if (LEVEL.equals("logic")) throw new IllegalStateException("the logic level has no picture");
    if (!name.matches("[\\w-]+")) throw new IllegalArgumentException("bad name " + name);
    Minecraft mc = ready();
    CompletableFuture<Object> saved = new CompletableFuture<>();
    File file = new File(new File(mc.gameDirectory, "screenshots"), name + ".png");
    Files.deleteIfExists(file.toPath());
    mc.execute(() -> Screenshot.grab(mc.gameDirectory, name + ".png", mc.getMainRenderTarget(), downscale, saved::complete));
    Object outcome = saved.get(10, TimeUnit.SECONDS);
    if (outcome instanceof Component message && message.getContents() instanceof TranslatableContents key && key.getKey().equals("screenshot.failure")) {
      throw new IllegalStateException(message.getString());
    }
    if (file.length() == 0) throw new IllegalStateException("empty capture " + file);
    return file.getPath();
  }

  private static void seq(String[] w) throws Exception {
    long pause = Long.parseLong(w[1]);
    for (String step : Arrays.copyOfRange(w, 2, w.length)) {
      String[] delta = step.split(",");
      onGame(() -> { player().turn(Double.parseDouble(delta[0]), Double.parseDouble(delta[1])); return 0; });
      Thread.sleep(pause);
    }
  }

  private static void key(Minecraft mc, int key, int action) throws Exception {
    Method press = mc.keyboardHandler.getClass().getDeclaredMethod("keyPress", long.class, int.class, KeyEvent.class);
    press.setAccessible(true);
    press.invoke(mc.keyboardHandler, mc.getWindow().handle(), action, new KeyEvent(key, 0, 0));
  }

  private static void hover(Minecraft mc, double x, double y) throws Exception {
    double scale = mc.getWindow().getGuiScale();
    for (Map.Entry<String, Double> axis : Map.of("xpos", x * scale, "ypos", y * scale).entrySet()) {
      Field field = mc.mouseHandler.getClass().getDeclaredField(axis.getKey());
      field.setAccessible(true);
      field.setDouble(mc.mouseHandler, axis.getValue());
    }
  }

  private static boolean click(Minecraft mc, double x, double y) {
    MouseButtonEvent event = new MouseButtonEvent(x, y, new MouseButtonInfo(0, 0));
    boolean handled = screen(mc).mouseClicked(event, false);
    screen(mc).mouseReleased(event);
    return handled;
  }

  private static Map<String, Object> dump(Minecraft mc) throws Exception {
    Map<String, Object> dump = fields();
    dump.put("vehicle", String.valueOf(player().getVehicle()));
    dump.put("camera_entity", String.valueOf(mc.getCameraEntity()));
    dump.put("hit", String.valueOf(mc.hitResult == null ? null : mc.hitResult.getType()));
    return dump;
  }

  private static List<String> entities(Minecraft mc) {
    List<String> all = new ArrayList<>();
    for (Entity entity : mc.level.entitiesForRendering()) {
      all.add(String.format(Locale.ROOT, "%d %s %.2f %.2f %.2f", entity.getId(), entity.getType().toShortString(), entity.getX(), entity.getY(), entity.getZ()));
    }
    return all;
  }

  private static void recording(Minecraft mc, String name, Writing task) {
    File dir = mc.gameDirectory.getParentFile();
    recording(new File(dir, name + ".txt"), new File(dir, name + ".done"), name, task);
  }

  /**
   * One recording of a name at a time: a second would share its file, and the first would sign
   * for it. The mark says a recording is whole: only its own thread knows when it ends.
   */
  static void recording(File file, File done, String name, Writing task) {
    GivenBack givenBack = new GivenBack();
    if (RECORDING.putIfAbsent(name, givenBack) != null) throw new IllegalStateException(name + " is still recording");
    done.delete();
    daemon("void-" + name, () -> {
      try (PrintWriter out = new PrintWriter(new FileWriter(file), true)) {
        Recorder recorder = new Recorder(out, givenBack);
        task.run(recorder);
        if (recorder.late) out.println(SPOILED);
      } finally {
        RECORDING.remove(name);
      }
      done.createNewFile();
    });
  }

  static void noteTimeGivenBack() {
    RECORDING.values().forEach(GivenBack::set);
  }

  private static void track(Minecraft mc, int samples) {
    Supplier<double[]> camera = () -> {
      Entity entity = mc.getCameraEntity();
      return entity == null ? null : new double[] {entity.getX(), entity.getZ()};
    };
    recording(mc, "track", out -> track(mc, camera, samples, out));
  }

  static void track(Executor game, Supplier<double[]> camera, int samples, Recorder out) throws Exception {
    double[] last = {Double.NaN};
    for (int i = 0; i < samples; i++) {
      game.execute(() -> {
        double[] at = camera.get();
        if (at != null && at[0] != last[0]) {
          last[0] = at[0];
          out.stamp(Util.getNanos() / 1_000_000 + " " + at[0] + " " + at[1]);
        }
      });
      Thread.sleep(2);
    }
    waitForGame(game);
  }

  private static void frames(Minecraft mc, int count) throws Exception {
    Field frames = Minecraft.class.getDeclaredField("frames");
    frames.setAccessible(true);
    recording(mc, "frames", out -> {
      int last = -1, seen = 0;
      while (seen < count) {
        int now = frames.getInt(mc);
        if (now != last) {
          last = now;
          seen++;
          mc.execute(() -> {
            var camera = mc.gameRenderer.getMainCamera();
            var at = camera.position();
            out.stamp(Util.getNanos() / 1000 + " " + at.x() + " " + at.z() + " " + at.y() + " " + mc.levelRenderer.countRenderedSections() + " " + mc.levelRenderer.hasRenderedAllSections() + " " + camera.yRot() + " " + camera.xRot());
          });
        }
        Thread.sleep(1);
      }
      waitForGame(mc);
    });
  }

  static void waitForGame(Executor game) throws Exception {
    CompletableFuture<Boolean> written = new CompletableFuture<>();
    game.execute(() -> written.complete(true));
    written.get(10, TimeUnit.SECONDS);
  }

  private static long messageSend() {
    return ObjCRuntime.getLibrary().getFunctionAddress("objc_msgSend");
  }

  private static long application() {
    return JNI.invokePPP(ObjCRuntime.objc_getClass("NSApplication"), ObjCRuntime.sel_getUid("sharedApplication"), messageSend());
  }

  private static void activationPolicy(long policy) {
    JNI.invokePPPP(application(), ObjCRuntime.sel_getUid("setActivationPolicy:"), policy, messageSend());
  }

  private static void show(Minecraft mc, boolean focus) {
    requirePicture();
    Window window = mc.getWindow();
    shown = true;
    if (focus) activationPolicy(0);
    GLFW.glfwShowWindow(window.handle());
    if (focus) {
      JNI.invokePPPP(application(), ObjCRuntime.sel_getUid("activateIgnoringOtherApps:"), 1L, messageSend());
      GLFW.glfwFocusWindow(window.handle());
    }
  }

  private static void hide(Minecraft mc) {
    requirePicture();
    GLFW.glfwHideWindow(mc.getWindow().handle());
    activationPolicy(2);
    shown = false;
  }

  private static void requirePicture() {
    if (LEVEL.equals("logic")) throw new IllegalStateException("the logic level has no window");
  }

  private static String ok(Map<String, Object> reply) {
    reply.putIfAbsent("version", VERSION);
    StringBuilder json = new StringBuilder("{\"ok\":true");
    for (Map.Entry<String, Object> entry : reply.entrySet()) {
      json.append(',').append(quote(entry.getKey())).append(':').append(value(entry.getValue()));
    }
    return json.append('}').toString();
  }

  private static String failure(String message) {
    return "{\"ok\":false,\"error\":" + quote(message) + ",\"version\":" + quote(VERSION) + "}";
  }

  private static String value(Object value) {
    if (value instanceof Number || value instanceof Boolean) return value.toString();
    if (value instanceof List<?> list) {
      List<String> items = new ArrayList<>();
      for (Object item : list) items.add(value(item));
      return "[" + String.join(",", items) + "]";
    }
    return quote(String.valueOf(value));
  }

  private static String quote(String text) {
    StringBuilder out = new StringBuilder("\"");
    for (char c : text.toCharArray()) {
      switch (c) {
        case '"' -> out.append("\\\"");
        case '\\' -> out.append("\\\\");
        case '\n' -> out.append("\\n");
        case '\r' -> out.append("\\r");
        case '\t' -> out.append("\\t");
        default -> {
          if (c < 0x20) out.append(String.format("\\u%04x", (int) c));
          else out.append(c);
        }
      }
    }
    return out.append('"').toString();
  }

  /** Time stands still and moves only on `step`, so a frame depends on the verbs and not on the machine. */
  static final class Stepping {
    private static final long SECOND = 1_000_000_000L;
    private static final long FENCE_LIMIT = 10 * SECOND;
    private static final long FENCE_POLL = 50_000L;
    /** A locked server sends nothing, and the game hangs up after this much silence. */
    private static final String TIMEOUT = "timeout";
    private static final int TIMEOUT_SECONDS = 30;
    /** Above zero, so that K frames of a tick add up to at least one tick despite float rounding. */
    private static final float PHASE = 1e-4f;
    static volatile TimeSource.NanoTimeSource real;
    private static final AtomicLong clock = new AtomicLong();
    private static long origin;
    private static double elapsedMs;
    private static final AtomicLong read = new AtomicLong();
    private static final AtomicLong written = new AtomicLong();
    private static volatile Channel channel;

    static boolean virtual() {
      return real != null;
    }

    static String clock(Minecraft mc, String mode) throws Exception {
      switch (mode) {
        case "virtual" -> {
          requireNoRecording();
          onGame(() -> { freeze(mc); return 0; });
        }
        case "real" -> {
          if (virtual()) onGame(() -> { release(mc); return 0; });
        }
        default -> throw new IllegalArgumentException("clock is virtual or real");
      }
      return mode;
    }

    static void requireNoRecording() {
      String name = RECORDING.keySet().stream().findFirst().orElse(null);
      if (name != null) {
        throw new IllegalStateException("clock virtual would freeze the time under the " + name + " still recording, and its check would fail: wait for its end");
      }
    }

    static boolean giveBackTime() {
      if (!virtual()) return false;
      noteTimeGivenBack();
      Util.timeSource = real;
      real = null;
      return true;
    }

    /** A whole second: the fractions of a millisecond then fall the same way on every run. */
    private static void freeze(Minecraft mc) throws Exception {
      if (virtual()) return;
      long now = Util.timeSource.getAsLong() / SECOND * SECOND;
      wire(channelOf(player().connection.getConnection()));
      start(now);
      restart(mc, now / 1_000_000L);
      real = Util.timeSource;
      Util.timeSource = clock::get;
    }

    private static void release(Minecraft mc) throws Exception {
      if (!giveBackTime()) return;
      restart(mc, Util.getMillis());
      rearm(wired());
    }

    private static void restart(Minecraft mc, long millis) throws Exception {
      DeltaTracker.Timer timer = (DeltaTracker.Timer) field(Minecraft.class, "deltaTracker").get(mc);
      field(DeltaTracker.Timer.class, "lastMs").setLong(timer, millis);
      field(DeltaTracker.Timer.class, "lastUiMs").setLong(timer, millis);
      field(DeltaTracker.Timer.class, "deltaTickResidual").setFloat(timer, PHASE);
      field(Minecraft.class, "lastTime").setLong(mc, millis);
    }

    private static Field field(Class<?> owner, String name) throws Exception {
      Field field = owner.getDeclaredField(name);
      field.setAccessible(true);
      return field;
    }

    static void start(long nanos) {
      origin = nanos;
      elapsedMs = 0;
      clock.set(nanos);
    }

    /** From the origin and not from the last frame: rounding each frame would lose nanoseconds. */
    static long advance(double ms) {
      elapsedMs += ms;
      long now = origin + Math.round(elapsedMs * 1_000_000);
      clock.set(now);
      return now;
    }

    private static Channel channelOf(Connection connection) throws Exception {
      return (Channel) field(Connection.class, "channel").get(connection);
    }

    static void rearm(Channel current) {
      if (current.pipeline().get(TIMEOUT) == null) current.pipeline().addFirst(TIMEOUT, new ReadTimeoutHandler(TIMEOUT_SECONDS));
    }

    static void wire(Channel current) {
      ChannelPipeline pipeline = current.pipeline();
      if (pipeline.get(TIMEOUT) != null) pipeline.remove(TIMEOUT);
      if (current == channel) return;
      read.set(0);
      written.set(0);
      pipeline.addAfter("splitter", "vt-in", new ChannelInboundHandlerAdapter() {
        @Override
        public void channelRead(ChannelHandlerContext context, Object message) {
          read.incrementAndGet();
          context.fireChannelRead(message);
        }
      });
      pipeline.addFirst("vt-out", new ChannelOutboundHandlerAdapter() {
        @Override
        public void write(ChannelHandlerContext context, Object message, ChannelPromise promise) {
          written.incrementAndGet();
          context.write(message, promise);
        }
      });
      channel = current;
    }

    /** `frames` goes back to 0 every second, so a frame is told by `lastTime` as well. */
    private static long frame(Minecraft mc) throws Exception {
      return field(Minecraft.class, "lastTime").getLong(mc) * 1_000_000L + field(Minecraft.class, "frames").getInt(mc);
    }

    /**
     * The game reads the clock before it runs tasks, so a task that moves the clock in frame N is
     * seen by frame N+1, and N+1 is only finished once a task runs in N+2. Frames in between see
     * the same time and draw the same thing.
     */
    static Map<String, Object> step(Minecraft mc, double ms, int count) throws Exception {
      if (!virtual()) throw new IllegalStateException("the clock is real: send clock virtual");
      Map<String, Object> camera = Map.of();
      for (int i = 0; i < count; i++) {
        long seen = onGame(() -> { advance(ms); return frame(mc); });
        for (int changes = 0; changes < 2; ) {
          long before = seen;
          Map<String, Object> now = onGame(() -> camera(mc));
          seen = (long) now.remove("frame");
          if (seen != before) changes++;
          camera = now;
        }
      }
      return camera;
    }

    private static Map<String, Object> camera(Minecraft mc) throws Exception {
      var camera = mc.gameRenderer.getMainCamera();
      var at = camera.position();
      Map<String, Object> reply = new LinkedHashMap<>();
      reply.put("frame", frame(mc));
      reply.put("x", at.x());
      reply.put("y", at.y());
      reply.put("z", at.z());
      reply.put("yaw", camera.yRot());
      reply.put("pitch", camera.xRot());
      return reply;
    }

    private static Channel wired() {
      Channel current = channel;
      if (current == null) throw new IllegalStateException("no counters: send clock virtual");
      return current;
    }

    private static void drain(Channel current) throws Exception {
      current.eventLoop().submit((Runnable) () -> {}).get(5, TimeUnit.SECONDS);
    }

    static long fence(long count) throws Exception {
      Channel current = wired();
      long start = System.nanoTime();
      while (read.get() < count) {
        if (!current.isActive()) throw new IllegalStateException("connection lost");
        if (System.nanoTime() - start > FENCE_LIMIT) throw new TimeoutException("fence expired: read " + read.get() + " of " + count);
        LockSupport.parkNanos(FENCE_POLL);
      }
      drain(current);
      return read.get();
    }

    static Map<String, Object> counters() throws Exception {
      Channel current = wired();
      boolean active = current.isActive();
      if (active) drain(current);
      Map<String, Object> reply = new LinkedHashMap<>();
      reply.put("in", read.get());
      reply.put("out", written.get());
      reply.put("active", active);
      return reply;
    }
  }
}
