import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

final class ConditionCases {
  public static void main(String[] args) throws Exception {
    Map<String, Map<String, Object>> levels = new HashMap<>();
    List<String> wrong = new ArrayList<>();
    int cases = 0;
    for (String line : Files.readAllLines(Path.of(args[0]), StandardCharsets.UTF_8)) {
      if (line.isEmpty() || line.startsWith("#")) continue;
      String[] cells = line.split("\t", -1);
      if (cells[0].equals("fields")) {
        if (cells.length < 4 || cells.length % 2 != 0) fail("a fields line holds a level and pairs: " + line);
        Map<String, Object> fields = new HashMap<>();
        for (int at = 2; at < cells.length; at += 2) fields.put(cells[at], value(cells[at + 1], line));
        levels.put(cells[1], fields);
        continue;
      }
      if (cells.length != 4) fail("a case has exactly four cells: " + line);
      if (!cells[0].equals("assert") && !cells[0].equals("wait")) fail("a case is an assert or a wait: " + line);
      if (!levels.containsKey(cells[1])) fail("no fields for the level of: " + line);
      cases++;
      String got = outcome(cells[0], cells[1], cells[2], levels.get(cells[1]));
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

  private static Object value(String cell, String line) {
    if (cell.length() >= 2 && cell.startsWith("\"") && cell.endsWith("\"")) return cell.substring(1, cell.length() - 1);
    if (cell.equals("true") || cell.equals("false")) return Boolean.parseBoolean(cell);
    if (cell.matches("-?[0-9]+")) return Long.parseLong(cell);
    if (cell.matches("-?[0-9]+\\.[0-9]+")) return Double.parseDouble(cell);
    fail("a value is quoted text, a number, true or false: " + line);
    return null;
  }

  private static String outcome(String kind, String level, String text, Map<String, Object> fields) {
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
      return "error";
    }
  }
}
