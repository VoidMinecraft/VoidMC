import io.netty.channel.ChannelInboundHandlerAdapter;
import io.netty.channel.embedded.EmbeddedChannel;
import io.netty.handler.timeout.ReadTimeoutHandler;
import java.io.File;
import java.io.IOException;
import java.nio.file.Files;
import java.util.List;
import java.util.concurrent.CopyOnWriteArrayList;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.atomic.AtomicBoolean;
import net.minecraft.util.TimeSource;
import net.minecraft.util.Util;

/** Runs without the game: the parts of the probe that only need a channel, a clock or a map. */
public class SteppingTest {
  private static final long TICK_NANOS = 50_000_000L;

  public static void main(String[] args) throws Exception {
    theTimeoutStaysOutWhenTheClockIsFrozenAgain();
    framesOfATickAddUpToTheTick(4);
    framesOfATickAddUpToTheTick(6);
    framesOfATickAddUpToTheTick(7);
    framesOfATickAddUpToTheTick(15);
    onlyAVirtualClockIsRefusedWhileARecordingRuns();
    aRecordingUnderAClockGivenBackSaysItIsInvalid();
    aRecordingIsOverBeforeItsMark();
    aRecordingWhoseLastLineCameBeforeTheTimeWentBackStaysWhole();
    givingBackTheTimeTellsTheRecordingsOnlyWhenItMovesIt();
    trackStampsOnTheGameThreadAndOnlyALineAfterTheTimeWentBackSpoilsIt();
  }

  private static void check(boolean holds, String what) {
    if (!holds) throw new AssertionError(what);
  }

  private static void theTimeoutStaysOutWhenTheClockIsFrozenAgain() {
    EmbeddedChannel channel = new EmbeddedChannel();
    channel.pipeline().addLast("timeout", new ReadTimeoutHandler(30));
    channel.pipeline().addLast("splitter", new ChannelInboundHandlerAdapter());
    Probe.Stepping.wire(channel);
    check(channel.pipeline().get("timeout") == null, "frozen: the timeout is out");
    Probe.Stepping.rearm(channel);
    check(channel.pipeline().get("timeout") != null, "real: the timeout is back");
    Probe.Stepping.wire(channel);
    check(channel.pipeline().get("timeout") == null, "frozen again: the timeout is out");
    check(channel.pipeline().get("vt-in") != null, "the counters are in");
  }

  private static void framesOfATickAddUpToTheTick(int frames) {
    long origin = 3_000_000_000L;
    Probe.Stepping.start(origin);
    long now = origin;
    for (int tick = 1; tick <= 1000; tick++) {
      for (int frame = 0; frame < frames; frame++) now = Probe.Stepping.advance(50.0 / frames);
      check(now == origin + tick * TICK_NANOS, frames + " frames: tick " + tick + " ends at " + (now - origin));
    }
  }


  private static void onlyAVirtualClockIsRefusedWhileARecordingRuns() throws Exception {
    Probe.Stepping.requireNoRecording();
    for (String name : new String[] {"frames", "track"}) {
      Probe.GivenBack givenBack = new Probe.GivenBack();
      Probe.RECORDING.put(name, givenBack);
      try {
        String refusal = null;
        try {
          Probe.Stepping.clock(null, "virtual");
        } catch (Exception refused) {
          refusal = String.valueOf(refused.getMessage());
        }
        check(refusal != null && refusal.contains("clock virtual would freeze the time under the " + name + " still recording"), "virtual while " + name + " records: " + refusal);
        check(Probe.Stepping.clock(null, "real").equals("real"), "real while " + name + " records");
        check(!givenBack.isSet(), "a clock already real moves no time under " + name);
      } finally {
        Probe.RECORDING.remove(name);
      }
    }
    Probe.Stepping.requireNoRecording();
  }

  private static File folder(String name) throws IOException {
    File dir = Files.createTempDirectory("stepping-" + name).toFile();
    dir.deleteOnExit();
    return dir;
  }

  private static void awaitMark(File done) throws InterruptedException {
    for (int i = 0; i < 500 && !done.exists(); i++) Thread.sleep(10);
    check(done.exists(), done + " never came");
  }

  private static void aRecordingUnderAClockGivenBackSaysItIsInvalid() throws Exception {
    File dir = folder("spoiled");
    File spoiled = new File(dir, "frames.txt");
    File done = new File(dir, "frames.done");
    Probe.recording(spoiled, done, "frames", out -> {
      out.stamp("1000 1 2 3");
      Probe.noteTimeGivenBack();
      out.stamp("2000 1 2 3");
    });
    awaitMark(done);
    List<String> lines = Files.readAllLines(spoiled.toPath());
    check(lines.equals(List.of("1000 1 2 3", "2000 1 2 3", Probe.SPOILED)), "spoiled: " + lines);
    File whole = new File(dir, "track.txt");
    File wholeDone = new File(dir, "track.done");
    Probe.recording(whole, wholeDone, "track", out -> out.stamp("1 2 3"));
    awaitMark(wholeDone);
    check(Files.readAllLines(whole.toPath()).equals(List.of("1 2 3")), "a recording left alone stays whole");
  }

  private static void aRecordingWhoseLastLineCameBeforeTheTimeWentBackStaysWhole() throws Exception {
    File dir = folder("whole");
    File file = new File(dir, "frames.txt");
    File done = new File(dir, "frames.done");
    Probe.recording(file, done, "frames", out -> {
      out.stamp("1000 1 2 3");
      out.stamp("2000 1 2 3");
      Probe.noteTimeGivenBack();
    });
    awaitMark(done);
    List<String> lines = Files.readAllLines(file.toPath());
    check(lines.equals(List.of("1000 1 2 3", "2000 1 2 3")), "every line came before: " + lines);
  }

  private static void givingBackTheTimeTellsTheRecordingsOnlyWhenItMovesIt() {
    TimeSource.NanoTimeSource original = Util.timeSource;
    TimeSource.NanoTimeSource frozen = () -> 1_000_000_000L;
    TimeSource.NanoTimeSource real = () -> 2_000_000_000L;
    AtomicBoolean toldWhileFrozen = new AtomicBoolean();
    Probe.GivenBack givenBack = new Probe.GivenBack() {
      @Override
      void set() {
        toldWhileFrozen.set(Util.timeSource == frozen);
        super.set();
      }
    };
    Probe.RECORDING.put("frames", givenBack);
    try {
      Util.timeSource = frozen;
      check(!Probe.Stepping.giveBackTime(), "a real clock has no time to give back");
      check(!givenBack.isSet(), "a real clock tells nothing");
      check(Util.timeSource == frozen, "a real clock keeps its source");
      Probe.Stepping.real = real;
      check(Probe.Stepping.giveBackTime(), "a virtual clock gives its time back");
      check(givenBack.isSet(), "the recording under it is told");
      check(toldWhileFrozen.get(), "the recording is told before the time goes back");
      check(Util.timeSource == real, "the real source is back");
      check(!Probe.Stepping.virtual(), "the clock is real again");
    } finally {
      Probe.RECORDING.remove("frames");
      Probe.Stepping.real = null;
      Util.timeSource = original;
    }
  }

  private static List<String> trackOnAGame(String name, double[][] positions, int givenBackAt, List<String> stampedOn, List<String> seenOn) throws Exception {
    ExecutorService game = Executors.newSingleThreadExecutor(task -> new Thread(task, "game"));
    TimeSource.NanoTimeSource original = Util.timeSource;
    Util.timeSource = () -> {
      stampedOn.add(Thread.currentThread().getName());
      return 7_000_000_000L;
    };
    try {
      File dir = folder(name);
      File file = new File(dir, "track.txt");
      File done = new File(dir, "track.done");
      int[] sample = {0};
      Probe.recording(file, done, "track", out -> Probe.track(game, () -> {
        seenOn.add(Thread.currentThread().getName());
        try {
          Thread.sleep(20);
        } catch (InterruptedException stopped) {
          throw new IllegalStateException(stopped);
        }
        if (sample[0] == givenBackAt) Probe.noteTimeGivenBack();
        return positions[sample[0]++];
      }, positions.length, out));
      awaitMark(done);
      return Files.readAllLines(file.toPath());
    } finally {
      Util.timeSource = original;
      game.shutdownNow();
    }
  }

  private static void trackStampsOnTheGameThreadAndOnlyALineAfterTheTimeWentBackSpoilsIt() throws Exception {
    List<String> stampedOn = new CopyOnWriteArrayList<>();
    List<String> seenOn = new CopyOnWriteArrayList<>();
    double[][] stopped = {{1, 0}, {2, 0}, {2, 0}, {2, 0}};
    List<String> whole = trackOnAGame("track-still", stopped, 2, stampedOn, seenOn);
    check(whole.equals(List.of("7000 1.0 0.0", "7000 2.0 0.0")), "a camera stopped before the time went back: " + whole);
    double[][] moving = {{1, 0}, {2, 0}, {3, 0}, {3, 0}};
    List<String> spoiled = trackOnAGame("track-moving", moving, 2, stampedOn, seenOn);
    check(spoiled.equals(List.of("7000 1.0 0.0", "7000 2.0 0.0", "7000 3.0 0.0", Probe.SPOILED)), "a line after the time went back: " + spoiled);
    check(seenOn.size() == 8 && seenOn.stream().allMatch("game"::equals), "the camera is read on the game thread: " + seenOn);
    check(stampedOn.size() == 5 && stampedOn.stream().allMatch("game"::equals), "every line is stamped on the game thread: " + stampedOn);
  }

  private static void aRecordingIsOverBeforeItsMark() throws Exception {
    File dir = folder("mark");
    AtomicBoolean stillRecording = new AtomicBoolean(true);
    File done = new File(dir, "frames.done") {
      @Override
      public boolean createNewFile() throws IOException {
        stillRecording.set(Probe.RECORDING.containsKey("frames"));
        return super.createNewFile();
      }
    };
    Probe.recording(new File(dir, "frames.txt"), done, "frames", out -> out.stamp("1 2 3"));
    awaitMark(done);
    check(!stillRecording.get(), "the mark came while frames still recorded");
  }
}
