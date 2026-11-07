import java.util.Map;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/** The rules of a condition, the same as condition.rs: conditions.tsv holds both to them. */
final class Condition {
  static final long DEFAULT_WAIT_MS = 20_000;
  private static final Pattern NUMBER = Pattern.compile("-?\\d+(\\.\\d+)?");
  private static final Pattern LIMIT = Pattern.compile("\\d{1,9}");
  private static final Pattern DIGITS = Pattern.compile("\\d+");
  private static final Pattern PARTS = Pattern.compile("^([A-Za-z][A-Za-z0-9_]*)\\s*(>=|<=|==|!=|~)\\s*(.*)$");

  record Wait(Matcher parts, long limit) {}

  private Condition() {}

  static Matcher parse(String condition) {
    Matcher parts = PARTS.matcher(condition.trim());
    if (!parts.matches() || parts.group(3).isBlank()) return null;
    boolean order = parts.group(2).equals(">=") || parts.group(2).equals("<=");
    if (order && !NUMBER.matcher(parts.group(3).trim()).matches()) return null;
    return parts;
  }

  static Matcher require(String condition) {
    Matcher parts = parse(condition);
    if (parts == null) {
      throw new IllegalArgumentException("bad condition " + condition + ": field op value, and >= or <= need a number");
    }
    return parts;
  }

  static Wait waitOf(String rest) {
    String text = rest.trim();
    int space = text.indexOf(' '), tab = text.indexOf('\t');
    int cut = space < 0 ? tab : tab < 0 ? space : Math.min(space, tab);
    if (cut > 0 && DIGITS.matcher(text.substring(0, cut)).matches()) {
      String first = text.substring(0, cut);
      if (!LIMIT.matcher(first).matches()) throw new IllegalArgumentException("bad limit " + first + ": 1 to 9 digits of milliseconds");
      return new Wait(require(text.substring(cut + 1)), Long.parseLong(first));
    }
    return new Wait(require(text), DEFAULT_WAIT_MS);
  }

  static boolean holds(String condition, Map<String, Object> fields) {
    return holds(require(condition), fields);
  }

  static boolean holds(Matcher parts, Map<String, Object> fields) {
    Object actual = fields.get(parts.group(1));
    if (actual == null) return false;
    String op = parts.group(2), expected = parts.group(3).trim();
    if (op.equals("~")) return actual.toString().contains(expected);
    boolean order = op.equals(">=") || op.equals("<=");
    if (actual instanceof Number number && NUMBER.matcher(expected).matches()) {
      int compared = Double.compare(number.doubleValue(), Double.parseDouble(expected));
      return switch (op) {
        case ">=" -> compared >= 0;
        case "<=" -> compared <= 0;
        case "==" -> compared == 0;
        default -> compared != 0;
      };
    }
    if (order) throw new IllegalArgumentException(parts.group(1) + " is " + actual + ", not a number");
    return op.equals("==") == actual.toString().equals(expected);
  }
}
