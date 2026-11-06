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
import java.util.concurrent.TimeUnit;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
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
import net.minecraft.world.entity.Entity;
import org.lwjgl.glfw.GLFW;
import org.lwjgl.system.JNI;
import org.lwjgl.system.macosx.ObjCRuntime;

/** Runs the game and answers verbs on a unix socket, one line in, one JSON line out. */
public class Probe {
  private static final Pattern CONDITION = Pattern.compile("^(\\w+)\\s*(>=|<=|==|!=|~)\\s*(.*)$");
  private static final String LEVEL = System.getProperty("void.level", "render");
  private static final String SERVER = System.getProperty("void.server");
  private static final long IDLE_MS = Long.getLong("void.idle.minutes", 30) * 60_000;
  private static final long DEFAULT_WAIT_MS = 20_000;
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

  private interface Task {
    void run() throws Exception;
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
      if (mc.screen != null && mc.screen.getClass().getSimpleName().equals("DisconnectedScreen")) {
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
      while ((line = in.readLine()) != null) out.println(answer(line.trim()));
    }
  }

  private static String answer(String line) {
    try {
      String[] words = line.split("\\s+");
      if (!words[0].equals("state") && !words[0].equals("thumb")) lastVerb = System.currentTimeMillis();
      return ok(verb(words[0], words, line));
    } catch (java.util.concurrent.ExecutionException error) {
      return failure(String.valueOf(error.getCause()));
    } catch (Throwable error) {
      return failure(String.valueOf(error));
    }
  }

  private static <T> T onGame(Callable<T> task) throws Exception {
    Minecraft mc = ready();
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

  private static Map<String, Object> verb(String verb, String[] w, String line) throws Exception {
    Minecraft mc = ready();
    String rest = line.contains(" ") ? line.substring(line.indexOf(' ') + 1) : "";
    Map<String, Object> reply = new LinkedHashMap<>();
    switch (verb) {
      case "state" -> reply.putAll(onGame(Probe::fields));
      case "act" -> onGame(() -> { player().connection.sendCommand(rest); return 0; });
      case "wait_until" -> reply.putAll(waitUntil(w));
      case "assert" -> {
        Map<String, Object> fields = onGame(Probe::fields);
        if (!holds(rest, fields)) throw new AssertionError(rest + " is false: " + fields);
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
      case "quit" -> mc.execute(mc::stop);
      default -> throw new IllegalArgumentException("unknown verb " + verb);
    }
    return reply;
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
    fields.put("fps", mc.getFps());
    fields.put("level", LEVEL);
    fields.put("reconnects", reconnects);
    fields.put("shown", shown);
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

  private static Map<String, Object> waitUntil(String[] w) throws Exception {
    long limit = w.length > 2 && w[w.length - 1].matches("\\d+") ? Long.parseLong(w[w.length - 1]) : DEFAULT_WAIT_MS;
    int end = w.length > 2 && w[w.length - 1].matches("\\d+") ? w.length - 1 : w.length;
    String condition = String.join(" ", Arrays.copyOfRange(w, 1, end));
    long start = System.currentTimeMillis();
    while (true) {
      try {
        Map<String, Object> fields = onGame(Probe::fields);
        if (holds(condition, fields)) {
          fields.put("waited_ms", System.currentTimeMillis() - start);
          return fields;
        }
      } catch (IllegalStateException notYet) {
      }
      if (System.currentTimeMillis() - start > limit) throw new AssertionError("timed out on " + condition);
      Thread.sleep(50);
    }
  }

  private static boolean holds(String condition, Map<String, Object> fields) {
    Matcher parts = CONDITION.matcher(condition.trim());
    if (!parts.matches()) throw new IllegalArgumentException("bad condition " + condition);
    Object actual = fields.get(parts.group(1));
    if (actual == null) return false;
    String op = parts.group(2), expected = parts.group(3).trim();
    if (op.equals("~")) return actual.toString().contains(expected);
    if (actual instanceof Number number && expected.matches("-?[\\d.]+")) {
      int order = Double.compare(number.doubleValue(), Double.parseDouble(expected));
      return switch (op) {
        case ">=" -> order >= 0;
        case "<=" -> order <= 0;
        case "==" -> order == 0;
        default -> order != 0;
      };
    }
    return op.equals("==") == actual.toString().equals(expected);
  }

  private static String shot(String name, int downscale) throws Exception {
    if (LEVEL.equals("logic")) throw new IllegalStateException("the logic level has no picture");
    if (!name.matches("[\\w-]+")) throw new IllegalArgumentException("bad name " + name);
    Minecraft mc = ready();
    CompletableFuture<Object> saved = new CompletableFuture<>();
    File file = new File(new File(mc.gameDirectory, "screenshots"), name + ".png");
    mc.execute(() -> Screenshot.grab(mc.gameDirectory, name + ".png", mc.getMainRenderTarget(), downscale, saved::complete));
    saved.get(10, TimeUnit.SECONDS);
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

  private static void track(Minecraft mc, int samples) {
    daemon("void-track", () -> {
      try (PrintWriter out = new PrintWriter(new FileWriter(new File(mc.gameDirectory.getParentFile(), "track.txt")), true)) {
        double last = Double.NaN;
        for (int i = 0; i < samples; i++) {
          Entity camera = mc.getCameraEntity();
          if (camera != null && camera.getX() != last) {
            last = camera.getX();
            out.println(System.nanoTime() / 1_000_000 + " " + camera.getX() + " " + camera.getZ());
          }
          Thread.sleep(2);
        }
      }
    });
  }

  private static void frames(Minecraft mc, int count) throws Exception {
    Field frames = Minecraft.class.getDeclaredField("frames");
    frames.setAccessible(true);
    daemon("void-frames", () -> {
      try (PrintWriter out = new PrintWriter(new FileWriter(new File(mc.gameDirectory.getParentFile(), "frames.txt")), true)) {
        int last = -1, seen = 0;
        while (seen < count) {
          int now = frames.getInt(mc);
          if (now != last) {
            last = now;
            seen++;
            mc.execute(() -> {
              var camera = mc.gameRenderer.getMainCamera();
              var at = camera.position();
              out.println(System.nanoTime() / 1000 + " " + at.x() + " " + at.z() + " " + at.y() + " " + mc.levelRenderer.countRenderedSections() + " " + mc.levelRenderer.hasRenderedAllSections() + " " + camera.yRot() + " " + camera.xRot());
            });
          }
          Thread.sleep(1);
        }
        Thread.sleep(100);
      }
    });
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
    StringBuilder json = new StringBuilder("{\"ok\":true");
    for (Map.Entry<String, Object> entry : reply.entrySet()) {
      json.append(',').append(quote(entry.getKey())).append(':').append(value(entry.getValue()));
    }
    return json.append('}').toString();
  }

  private static String failure(String message) {
    return "{\"ok\":false,\"error\":" + quote(message) + "}";
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
}
