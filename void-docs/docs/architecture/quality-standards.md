# Quality Standards & Automation

VoidMC uses the Rust toolchain as its primary quality-analysis platform. For this ecosystem, `rustc`, Clippy, rustfmt, Cargo tests, and GitHub Actions cover the same role that a generic SonarQube-style setup would often cover in other stacks.

## Code Quality Tools

| Tool | Command | Purpose |
|---|---|---|
| rustfmt | `cargo fmt --all -- --check` | Enforces consistent formatting across the workspace. |
| Clippy | `cargo clippy --workspace --all-targets --all-features -- -D warnings` | Performs static analysis and treats warnings as merge-blocking errors. |
| Cargo tests | `cargo test --workspace --all-features` | Runs unit, integration, and doc tests. |
| cargo-llvm-cov | CI coverage job | Measures line/region/function coverage, displays total coverage in the job summary, and uploads LCOV artifacts. |
| Rust compiler | Included in build/test/clippy | Enforces ownership, lifetimes, type safety, and API correctness. |
| Rspress build | `npm run build` from `void-docs/` | Validates the documentation site and navigation. |

## Continuous Integration

The CI workflow lives at `.github/workflows/ci.yml` and runs on pushes and pull requests to `main`.

| Job | Check |
|---|---|
| `rustfmt` | `cargo fmt --all -- --check` |
| `clippy` | `cargo clippy --workspace --all-targets --all-features -- -D warnings` |
| `test` | `cargo test --workspace --all-features` |
| `coverage` | `cargo llvm-cov --workspace --all-features` with text summary and LCOV artifact upload |

The workflow also sets `RUSTFLAGS="-D warnings"` so compiler warnings are treated as failures.

The docs workflow lives at `.github/workflows/docs.yml` and validates the documentation site separately.

## Validation Evidence

| Command | What it proves |
|---|---|
| `cargo fmt --all -- --check` | Code style is reproducible and no formatting drift exists. |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | Static analysis passes across library, binary, test, and benchmark targets. |
| `cargo test --workspace --all-features` | Critical modules such as codec, protocol, data, commands, and world behavior pass regression tests. |
| CI coverage job | Displays total coverage in the GitHub Actions summary and uploads full text plus `lcov.info` artifacts for review or external tools. |
| `npm run build` in `void-docs/` | Documentation pages and navigation build successfully. |

## Test Coverage Focus

VoidMC prioritizes tests around protocol correctness and core framework behavior:

- `void-codec`: primitive encoding/decoding, VarInt/VarLong behavior, derive attributes, tagged enums, fixed-length fields, remaining payloads.
- `void-protocol`: packet encoding for critical clientbound/serverbound structures.
- `void-data`: generated block/entity data validation.
- `void`: command parsing, summon validation, entity movement encoding, world/chunk coordinate behavior.

## Reliability Standards

- Cross-thread communication must go through flume channels.
- ECS world mutation belongs on the game thread.
- New protocol behavior should include encode/decode tests when practical.
- New public API behavior should update docs or examples.
- Runtime logs should use `tracing`, not ad hoc output.
- Generated runtime output belongs under `logs/` and must not be committed.

## Review Checklist

Before merging a meaningful change:

1. Run formatting, Clippy, and tests.
2. Update docs/examples for public API changes.
3. Add tests for behavior changes.
4. Keep unrelated refactors out of the change.
5. Document operational or security limitations honestly when a feature is incomplete.
