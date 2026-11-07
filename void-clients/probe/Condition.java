import java.util.List;
import java.util.Map;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

final class Condition {
  static final long DEFAULT_WAIT_MS = 20_000;
  static final List<String> TEXT_FIELDS = List.of(
      "action_bar",
      "clock",
      "state",
      "screen",
      "level",
      "chat",
      "dimension",
      "last_error",
      "passenger",
      "spectator",
      "shown",
      "version");
  private static final int[][] HIDDEN = {
      {0x0000, 0x0008},
      {0x000A, 0x001F},
      {0x007F, 0x00A0},
      {0x00AD, 0x00AD},
      {0x0600, 0x0605},
      {0x061C, 0x061C},
      {0x06DD, 0x06DD},
      {0x070F, 0x070F},
      {0x0890, 0x0891},
      {0x08E2, 0x08E2},
      {0x1680, 0x1680},
      {0x180E, 0x180E},
      {0x2000, 0x200F},
      {0x2028, 0x202F},
      {0x205F, 0x2064},
      {0x2066, 0x206F},
      {0x3000, 0x3000},
      {0xFEFF, 0xFEFF},
      {0xFFF9, 0xFFFB},
      {0x110BD, 0x110BD},
      {0x110CD, 0x110CD},
      {0x13430, 0x1343F},
      {0x1BCA0, 0x1BCA3},
      {0x1D173, 0x1D17A},
      {0xE0001, 0xE0001},
      {0xE0020, 0xE007F}};
  private static final Pattern NUMBER = Pattern.compile("-?[0-9]+(\\.[0-9]+)?");
  private static final Pattern LIMIT = Pattern.compile("[0-9]{1,9}");
  private static final Pattern DIGITS = Pattern.compile("[0-9]+");
  private static final Pattern PARTS = Pattern.compile("^([A-Za-z][A-Za-z0-9_]*)[ \t]*(>=|<=|==|!=|~)(.*)$", Pattern.DOTALL);

  record Parsed(String field, String op, String expected) {
    boolean orders() {
      return op.equals(">=") || op.equals("<=");
    }
  }

  record Wait(Parsed parsed, long limit) {}

  interface Fields {
    Map<String, Object> read(long withinMs) throws Exception;
  }

  private Condition() {}

  private static boolean blank(char c) {
    return c == ' ' || c == '\t';
  }

  private static String trimmed(String text) {
    int start = 0, end = text.length();
    while (start < end && blank(text.charAt(start))) start++;
    while (end > start && blank(text.charAt(end - 1))) end--;
    return text.substring(start, end);
  }

  private static boolean hasBlank(String text) {
    return firstBlank(text) >= 0;
  }

  private static int firstBlank(String text) {
    for (int at = 0; at < text.length(); at++) if (blank(text.charAt(at))) return at;
    return -1;
  }

  static boolean hidden(int c) {
    for (int[] range : HIDDEN) if (range[0] <= c && c <= range[1]) return true;
    return false;
  }

  static String[] split(String line) {
    String text = trimmed(line);
    int cut = firstBlank(text);
    if (cut < 0) return new String[] {text, ""};
    return new String[] {text.substring(0, cut), trimmed(text.substring(cut + 1))};
  }

  private static String valueOf(String raw, String text) {
    if (raw.startsWith("\"")) {
      String inner = raw.length() >= 2 && raw.endsWith("\"") ? raw.substring(1, raw.length() - 1) : null;
      if (inner == null || inner.contains("\"")) throw new IllegalArgumentException(text + ": a quoted value opens and closes with one \" each");
      if (inner.isEmpty()) throw new IllegalArgumentException(text + " compares with nothing");
      return inner;
    }
    if (raw.isEmpty()) throw new IllegalArgumentException(text + " compares with nothing");
    if (hasBlank(raw)) {
      throw new IllegalArgumentException(text + ": a value is one word, so quote one with spaces, field~\"a b\", and give a wait its limit first, wait_until 5000 condition");
    }
    if (raw.contains("\"")) throw new IllegalArgumentException(text + ": a quote goes around the whole value");
    return raw;
  }

  static Parsed parse(String text) {
    if (text.codePoints().anyMatch(Condition::hidden)) {
      throw new IllegalArgumentException(text + ": a condition holds visible characters, spaces and tabs, nothing else");
    }
    Matcher parts = PARTS.matcher(trimmed(text));
    if (!parts.matches()) throw new IllegalArgumentException("bad condition " + text);
    String expected = valueOf(trimmed(parts.group(3)), text);
    Parsed parsed = new Parsed(parts.group(1), parts.group(2), expected);
    if (parsed.orders() && !NUMBER.matcher(expected).matches()) {
      throw new IllegalArgumentException(text + ": " + parsed.op() + " needs a number, not " + expected);
    }
    return parsed;
  }

  static void validate(Parsed parsed, String level) {
    boolean text = TEXT_FIELDS.contains(parsed.field()) || (parsed.field().equals("camera") && !level.equals("bot"));
    if (parsed.orders() && text) {
      throw new IllegalArgumentException(parsed.field() + " is not a number: " + parsed.op() + " cannot compare it");
    }
  }

  static Wait waitOf(String rest) {
    String text = trimmed(rest);
    int cut = firstBlank(text);
    if (cut > 0 && DIGITS.matcher(text.substring(0, cut)).matches()) {
      String first = text.substring(0, cut);
      if (!LIMIT.matcher(first).matches()) throw new IllegalArgumentException("bad limit " + first + ": 1 to 9 digits of milliseconds");
      return new Wait(parse(text.substring(cut + 1)), Long.parseLong(first));
    }
    return new Wait(parse(text), DEFAULT_WAIT_MS);
  }

  static Map<String, Object> await(Wait wait, String text, Fields source, long pauseMs) throws Exception {
    long start = System.currentTimeMillis();
    while (true) {
      try {
        long left = Math.max(1, wait.limit() - (System.currentTimeMillis() - start));
        Map<String, Object> fields = source.read(left);
        if (holds(wait.parsed(), fields)) {
          fields.put("waited_ms", System.currentTimeMillis() - start);
          return fields;
        }
      } catch (IllegalStateException | java.util.concurrent.TimeoutException notYet) {
      }
      if (System.currentTimeMillis() - start > wait.limit()) throw new AssertionError("timed out on " + text);
      Thread.sleep(pauseMs);
    }
  }

  static boolean holds(Parsed parsed, Map<String, Object> fields) {
    Object actual = fields.get(parsed.field());
    if (actual == null) return false;
    String text = actual.toString(), expected = parsed.expected();
    if (parsed.op().equals("~")) return text.contains(expected);
    if (actual instanceof Number number && NUMBER.matcher(expected).matches()) {
      int compared = Double.compare(number.doubleValue(), Double.parseDouble(expected));
      return switch (parsed.op()) {
        case ">=" -> compared >= 0;
        case "<=" -> compared <= 0;
        case "==" -> compared == 0;
        default -> compared != 0;
      };
    }
    if (parsed.orders()) throw new IllegalArgumentException(parsed.field() + " is " + actual + ", not a number");
    return parsed.op().equals("==") == text.equals(expected);
  }
}
