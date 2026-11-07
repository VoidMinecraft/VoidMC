import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

final class ConditionCases {
  public static void main(String[] args) throws Exception {
    if (args[0].equals("--await")) {
      int[] tries = {0};
      long[] widest = {0};
      Map<String, Object> loaded = new HashMap<>(Map.of("chunks", 1000L));
      Condition.Fields loading = within -> {
        widest[0] = Math.max(widest[0], within);
        if (++tries[0] <= 2) throw new IllegalStateException("game not ready");
        if (tries[0] <= 3) throw new java.util.concurrent.TimeoutException();
        return new HashMap<>(loaded);
      };
      Condition.await(Condition.waitOf("2000 chunks>=1000"), "chunks>=1000", loading, 1);
      System.out.println("ready after " + tries[0] + " tries, within " + (widest[0] <= 2000));
      try {
        Condition.await(Condition.waitOf("200 chunks>=1"), "chunks>=1", within -> { throw new IllegalStateException("game not ready"); }, 1);
      } catch (AssertionError timedOut) {
        System.out.println(timedOut.getMessage());
      }
      return;
    }
    if (args[0].equals("--retry")) {
      Retry retry = new Retry(500, 2000);
      Object[] s = new Object[8];
      for (int i = 0; i < s.length; i++) s[i] = new Object();
      int[] asks = {0};
      java.util.function.BooleanSupplier open = () -> { asks[0]++; return true; };
      java.util.function.BooleanSupplier closed = () -> { asks[0]++; return false; };
      Runnable done = () -> retry.task(() -> {}).run();
      List<String> seen = new ArrayList<>();
      seen.add("away " + retry.look(0, false, s[0], closed) + " " + retry.look(100, false, s[0], closed)
          + " back " + retry.look(200, false, s[0], open));
      seen.add("queued " + retry.look(300, false, s[0], open));
      done.run();
      seen.add("soon " + retry.look(400, false, s[0], open) + " later " + retry.look(700, false, s[0], open));
      seen.add("never ran " + retry.look(1300, false, s[0], open) + " " + retry.look(5100, false, s[0], open)
          + " stale " + retry.look(5700, false, s[0], open));
      try {
        retry.task(() -> { throw new IllegalStateException("failed"); }).run();
      } catch (IllegalStateException failed) {
        seen.add("after a failure " + retry.look(6200, false, s[0], open));
      }
      done.run();
      asks[0] = 0;
      seen.add("kicked " + retry.look(6300, false, s[1], open) + " " + retry.look(7100, false, s[1], open)
          + " " + retry.look(7200, false, s[1], open) + " asked " + asks[0]);
      done.run();
      retry.look(7300, true, null, open);
      retry.look(7800, true, null, open);
      retry.look(8200, true, null, open);
      String steady = "steady " + retry.look(8300, false, s[2], open);
      done.run();
      seen.add(steady + " " + retry.look(8800, false, s[2], open));
      done.run();
      seen.add("kicked after a steady game " + retry.look(8900, false, s[3], open) + " " + retry.look(9700, false, s[3], open)
          + " " + retry.look(9800, false, s[3], open));
      done.run();
      seen.add("kicked again " + retry.look(9900, false, s[4], open) + " " + retry.look(11700, false, s[4], open)
          + " " + retry.look(11800, false, s[4], open));
      done.run();
      seen.add("longest " + retry.look(11900, false, s[5], open) + " " + retry.look(13799, false, s[5], open)
          + " " + retry.look(13800, false, s[5], open));
      done.run();
      seen.add("server away " + retry.look(13900, false, s[6], closed) + " " + retry.look(14000, false, s[6], closed)
          + " back " + retry.look(14100, false, s[6], open));
      done.run();
      retry.look(14200, true, null, open);
      retry.look(14700, true, null, open);
      String brief = "brief game " + retry.look(15199, false, s[7], open);
      done.run();
      seen.add(brief + " " + retry.look(15799, false, s[7], open) + " " + retry.look(16199, false, s[7], open));
      System.out.println(String.join("\n", seen));
      return;
    }
    if (args[0].equals("--hidden")) {
      for (int c = 0; c <= Character.MAX_CODE_POINT; c++) {
        if ((c < 0xD800 || c > 0xDFFF) && Condition.hidden(c)) System.out.println(Integer.toHexString(c));
      }
      return;
    }
    Map<String, Map<String, Object>> levels = new HashMap<>();
    List<String> wrong = new ArrayList<>();
    int cases = 0;
    for (String line : Files.readAllLines(Path.of(args[0]), StandardCharsets.UTF_8)) {
      if (line.isEmpty() || line.startsWith("#")) continue;
      String[] cells = line.split("\t", -1);
      for (int at = 0; at < cells.length; at++) cells[at] = unescape(cells[at], line);
      if (cells[0].equals("fields")) {
        if (cells.length < 4 || cells.length % 2 != 0) fail("a fields line holds a level and pairs: " + line);
        Map<String, Object> fields = new HashMap<>();
        for (int at = 2; at < cells.length; at += 2) fields.put(cells[at], value(cells[at + 1], line));
        levels.put(cells[1], fields);
        continue;
      }
      if (cells.length != 4 && !(cells.length == 5 && cells[3].equals("error") && !cells[4].isEmpty())) fail("a case has four cells, and an error a fifth, not empty, for a piece of its message: " + line);
      if (!List.of("assert", "wait", "split").contains(cells[0])) fail("a case is an assert, a wait or a split: " + line);
      if (!levels.containsKey(cells[1])) fail("no fields for the level of: " + line);
      cases++;
      String got = outcome(cells[0], cells[1], cells[2], levels.get(cells[1]), cells.length == 5 ? cells[4] : null);
      if (!got.equals(cells[3])) wrong.add(line + "\tbut the probe says " + got);
    }
    wrong.forEach(System.out::println);
    System.out.println(cases + " cases, " + wrong.size() + " wrong");
    System.exit(wrong.isEmpty() && cases > 0 ? 0 : 1);
  }

  private static void fail(String why) {
    System.out.println(why);
    System.exit(2);
  }

  private static String unescape(String cell, String line) {
    StringBuilder out = new StringBuilder();
    for (int at = 0; at < cell.length(); at++) {
      char c = cell.charAt(at);
      if (c != '\\') {
        out.append(c);
        continue;
      }
      char next = at + 1 < cell.length() ? cell.charAt(at + 1) : ' ';
      if (next == 't' || next == '\\') {
        out.append(next == 't' ? '\t' : '\\');
        at++;
      } else if (next == 'u' && at + 6 <= cell.length() && cell.substring(at + 2, at + 6).matches("[0-9A-Fa-f]{4}")) {
        out.append((char) Integer.parseInt(cell.substring(at + 2, at + 6), 16));
        at += 5;
      } else {
        fail("an escape is \\t, \\\\ or \\uXXXX: " + line);
      }
    }
    return out.toString();
  }

  private static Object value(String cell, String line) {
    if (cell.length() >= 2 && cell.startsWith("\"") && cell.endsWith("\"")) return cell.substring(1, cell.length() - 1);
    if (cell.equals("true") || cell.equals("false")) return Boolean.parseBoolean(cell);
    if (cell.matches("-?[0-9]+")) return Long.parseLong(cell);
    if (cell.matches("-?[0-9]+\\.[0-9]+")) return Double.parseDouble(cell);
    fail("a value is quoted text, a number, true or false: " + line);
    return null;
  }

  private static String outcome(String kind, String level, String text, Map<String, Object> fields, String fragment) {
    if (kind.equals("split")) {
      String[] parts = Condition.split(text);
      return parts[0] + "|" + parts[1];
    }
    try {
      if (kind.equals("assert")) {
        Condition.Parsed parsed = Condition.parse(text);
        Condition.validate(parsed, level);
        return String.valueOf(Condition.holds(parsed, fields));
      }
      Condition.Wait wait = Condition.waitOf(text);
      Condition.validate(wait.parsed(), level);
      return wait.limit() + " " + Condition.holds(wait.parsed(), fields);
    } catch (IllegalArgumentException refused) {
      String message = String.valueOf(refused.getMessage());
      return fragment == null || message.contains(fragment) ? "error" : "an error without \"" + fragment + "\": " + message;
    }
  }
}
