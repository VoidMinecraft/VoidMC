# Testing Guide

Comprehensive testing is essential for maintaining code quality and preventing regressions. This guide explains how to write and run tests in Void.

## Test Organization

Tests are organized into three categories:

### Unit Tests

Located within the source code files, right next to the code they test.

```rust
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add() {
        assert_eq!(add(2, 3), 5);
    }
}
```

### Integration Tests

Located in the `tests/` directory, testing the public API.

```
crate/
├── src/
│   ├── lib.rs
│   └── main.rs
└── tests/
    ├── common.rs
    ├── integration_test_1.rs
    └── integration_test_2.rs
```

### Documentation Tests

Tests embedded in documentation comments.

````rust
/// Adds two numbers.
///
/// # Examples
///
/// ```
/// use voidmc::add;
/// assert_eq!(add(2, 3), 5);
/// ```
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}
````

## Running Tests

### Basic Commands

```bash
# Run all tests
cargo test

# Run tests for specific crate
cargo test -p void-codec

# Run specific test by name
cargo test test_varint

# Run with output visible
cargo test -- --nocapture

# Run single-threaded (easier debugging)
cargo test -- --test-threads=1

# Run ignored tests only
cargo test -- --ignored

# Run all including ignored
cargo test -- --include-ignored
```

### Advanced Options

```bash
# Show test names without running
cargo test --list

# Run with environment variables
RUST_BACKTRACE=full cargo test

# Generate coverage
cargo tarpaulin --out Html

# Benchmark
cargo bench
```

## Writing Good Tests

### Principles

1. **Arrange-Act-Assert Pattern**

```rust
#[test]
fn test_decode_packet() {
    // Arrange
    let mut buffer = vec![0x03, 0x00, 0x01];

    // Act
    let result = Packet::decode(&mut buffer);

    // Assert
    assert!(result.is_ok());
}
```

2. **One Assertion Per Test** (when possible)

```rust
// Good: focused test
#[test]
fn test_player_name_is_stored() {
    let player = Player::new("Alice".to_string());
    assert_eq!(player.name, "Alice");
}

// Also good: multiple related assertions
#[test]
fn test_player_creation() {
    let player = Player::new("Alice".to_string());
    assert_eq!(player.name, "Alice");
    assert_eq!(player.position, Position::default());
}

// Less good: testing multiple independent things
#[test]
fn test_everything() {
    // player tests
    // entity tests
    // world tests
}
```

3. **Descriptive Names**

```rust
// Good
#[test]
fn decode_varint_with_value_zero_returns_zero() { }

#[test]
fn encode_string_with_empty_input_produces_zero_length() { }

// Less clear
#[test]
fn test_varint() { }

#[test]
fn test_string() { }
```

### Example Test Suite

```rust
#[cfg(test)]
mod tests {
    use super::*;

    mod encode {
        use super::*;

        #[test]
        fn varint_zero() {
            let mut buf = Vec::new();
            0i32.encode(&mut buf);
            assert_eq!(buf, vec![0x00]);
        }

        #[test]
        fn varint_127() {
            let mut buf = Vec::new();
            127i32.encode(&mut buf);
            assert_eq!(buf, vec![0x7f]);
        }

        #[test]
        fn varint_128() {
            let mut buf = Vec::new();
            128i32.encode(&mut buf);
            assert_eq!(buf, vec![0x80, 0x01]);
        }
    }

    mod decode {
        use super::*;

        #[test]
        fn varint_single_byte() {
            let mut buf = &[0x05][..];
            let value = i32::decode(&mut buf).unwrap();
            assert_eq!(value, 5);
        }

        #[test]
        fn varint_overflow_returns_error() {
            let mut buf = &[0xFF, 0xFF, 0xFF, 0xFF, 0xFF][..];
            let result = i32::decode(&mut buf);
            assert!(result.is_err());
        }
    }
}
```

## Testing Different Components

### Codec Testing (void-codec)

Focus on serialization/deserialization:

```rust
#[test]
fn roundtrip_packet_data() {
    let original = SomePacket { /* ... */ };

    // Encode
    let mut buffer = Vec::new();
    original.encode(&mut buffer);

    // Decode
    let mut reader = &buffer[..];
    let decoded = SomePacket::decode(&mut reader).unwrap();

    // Verify
    assert_eq!(original, decoded);
}
```

### Protocol Testing (void-protocol)

Test packet definitions and state transitions:

```rust
#[test]
fn play_state_accepts_move_packet() {
    let packet = PlayPacket::PlayerMovement { /* ... */ };
    let state = State::Play;

    assert!(is_valid_packet_for_state(&packet, state));
}

#[test]
fn status_state_rejects_play_packets() {
    let packet = PlayPacket::PlayerMovement { /* ... */ };
    let state = State::Status;

    assert!(!is_valid_packet_for_state(&packet, state));
}
```

### Async Testing

Use `#[tokio::test]` for async code:

```rust
#[tokio::test]
async fn client_receives_welcome_message() {
    let server = start_test_server().await;
    let mut client = connect_to_server(&server).await;

    let message = client.read_next_packet().await.unwrap();
    assert_eq!(message.text, "Welcome to Void!");
}

#[tokio::test]
async fn concurrent_clients_dont_interfere() {
    let server = start_test_server().await;

    let clients: Vec<_> = (0..10)
        .map(|_| connect_to_server(&server))
        .collect();

    // Verify all are connected
    assert_eq!(clients.len(), 10);
}
```

## Testing Best Practices

### What to Test

✅ **Must test:**

- Edge cases (0, negative, overflow, max value)
- Error conditions
- State transitions
- Critical paths

✅ **Should test:**

- Typical use cases
- Integration points
- Performance-critical code

### What NOT to Overly Test

❌ **Don't test:**

- Third-party library behavior (they have their own tests)
- Trivial getter functions
- The compiler (trust Rust's type system)

### Coverage Goals

Aim for coverage on:

- **Critical code**: 95%+ (codec, protocol parsing)
- **Core logic**: 80%+ (game state, handlers)
- **Utilities**: 70%+ (helpers, formatting)

```bash
# Generate coverage report
cargo tarpaulin --out Html
```

## Debugging Tests

### Getting More Information

```bash
// Enable backtrace
RUST_BACKTRACE=1 cargo test

// Show println! output
cargo test -- --nocapture

// Run single-threaded for deterministic output
cargo test -- --test-threads=1
```

### Using println! in Tests

```rust
#[test]
fn test_with_debug_output() {
    let result = some_function();
    println!("Result: {:?}", result);  // Shows when test fails
}
```

### Interactive Debugging

```bash
# Run test in debugger
rust-gdb --args cargo test test_name -- --test-threads=1
```

## Continuous Integration

All tests run automatically on:

- Every commit push
- Every pull request
- Before merge to main

Tests must pass before code can be merged.

## Common Test Patterns

### Testing Result Types

```rust
#[test]
fn valid_input_returns_ok() {
    let result = parse("valid");
    assert!(result.is_ok());
}

#[test]
fn invalid_input_returns_err() {
    let result = parse("invalid");
    assert!(result.is_err());
}

#[test]
fn error_message_is_descriptive() {
    let result = parse("invalid").unwrap_err();
    assert!(result.contains("expected"));
}
```

### Testing Panic Behavior

```rust
#[test]
#[should_panic(expected = "divide by zero")]
fn divide_by_zero_panics() {
    divide(10, 0);
}
```

### Testing with Fixtures

```rust
fn setup() -> TestEnvironment {
    TestEnvironment::new()
}

#[test]
fn test_with_setup() {
    let env = setup();
    // Use env...
}
```

## Test Maintenance

- Keep tests synchronized with code changes
- Remove obsolete tests
- Refactor duplicate test code
- Update test names if behavior changes
