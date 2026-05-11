# Quality Standards & Automation

To guarantee the reliability, security, and maintainability of our codebase, we enforce strict quality standards automatically during the development cycle. This satisfies our objective to structure and reliable the technical architecture.

## Code Quality Tools

Instead of relying on external tools like SonarQube which are often tailored for other ecosystems, the Rust ecosystem provides powerful, built-in equivalents that we mandate across the entire workspace:

### 1. Clippy (Linter)
`cargo clippy` acts as our primary static analysis tool. It catches common mistakes, performance pitfalls, and non-idiomatic code.
* **Standard:** We run Clippy with `-D warnings`. This means any warning emitted by Clippy is treated as a hard compilation error. Code with warnings cannot be merged.
* **Usage:** `cargo clippy --all-targets --all-features -- -D warnings`

### 2. Rustfmt (Formatter)
`cargo fmt` enforces a consistent code style across all crates in the workspace.
* **Standard:** All code is formatted using the default Rustfmt configuration.
* **Usage:** `cargo fmt --all`

### 3. Cargo Tests (Unit Validation)
Our multi-crate architecture enforces separation of concerns, making unit testing easier.
* **Protocol & Codecs:** Heavy unit testing guarantees that `void-codec` and `void-protocol` precisely encode and decode packets according to the Minecraft specifications.
* **Usage:** `cargo test --workspace`

## Continuous Integration (CI)

We automate our quality checks using GitHub Actions. Upon every Pull Request (PR) or push to the `main` branch, the CI pipeline verifies:
1. **Formatting:** Steps fail if `cargo fmt` detects unformatted code.
2. **Linting:** Steps fail if `cargo clippy` detects any lints (`deny(warnings)` rule).
3. **Tests:** Steps fail if unit or integration tests fail (`cargo test`).

This guarantees that `main` is always in a deployable, robust state, and reduces the review burden on developers by automating stylistic and common mistake detection.
