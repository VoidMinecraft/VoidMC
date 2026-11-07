import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

/** Runs conditions.tsv against Condition.java; cargo test runs it. */
final class ConditionCases {
  public static void main(String[] args) throws Exception {
    Map<String, Map<String, Object>> levels = new HashMap<>();
    List<String> wrong = new ArrayList<>();
    int cases = 0;
    for (String line : Files.readAllLines(Path.of(args[0]))) {
      if (line.isBlank() || line.startsWith("#")) continue;
      String[] cells = line.split("\t", -1);
      if (cells[0].equals("fields")) {
        Map<String, Object> fields = new HashMap<>();
        for (int at = 2; at + 1 < cells.length; at += 2) fields.put(cells[at], value(cells[at + 1]));
        levels.put(cells[1], fields);
        continue;
      }
      cases++;
      String got = outcome(cells[0], cells[2], levels.get(cells[1]));
      if (!got.equals(cells[3])) wrong.add(line + "\tbut the probe says " + got);
    }
    wrong.forEach(System.out::println);
    System.out.println(cases + " cases, " + wrong.size() + " wrong");
    System.exit(wrong.isEmpty() && cases > 0 ? 0 : 1);
  }

  private static Object value(String cell) {
    if (cell.startsWith("\"")) return cell.substring(1, cell.length() - 1);
    if (cell.equals("true") || cell.equals("false")) return Boolean.parseBoolean(cell);
    return cell.contains(".") ? (Object) Double.parseDouble(cell) : (Object) Long.parseLong(cell);
  }

  private static String outcome(String kind, String text, Map<String, Object> fields) {
    try {
      if (kind.equals("assert")) return String.valueOf(Condition.holds(text, fields));
      Condition.Wait wait = Condition.waitOf(text);
      return wait.limit() + " " + Condition.holds(wait.parts(), fields);
    } catch (RuntimeException refused) {
      return "error";
    }
  }
}
