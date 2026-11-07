import java.util.HashSet;
import java.util.Set;

final class Guard {
  private static final int REMEMBERED = 32;
  private final Set<String> seen = new HashSet<>();

  Throwable run(Runnable body) {
    try {
      body.run();
      return null;
    } catch (Throwable error) {
      String key;
      try {
        key = String.valueOf(error);
      } catch (Throwable unnamed) {
        key = error.getClass().getName();
      }
      if (seen.contains(key) || seen.size() >= REMEMBERED) return null;
      seen.add(key);
      return error;
    }
  }
}
