# Contributing to Void

Thanks for your interest in contributing to Void.

Void is a modular Minecraft server framework in Rust. We prioritize performance, maintainability, and clean APIs.

## Quick Start

1. Fork the repository.
2. Create a feature branch from `main`.
3. Make focused changes (one concern per PR).
4. Run checks locally:
   - `cargo fmt --all`
   - `cargo clippy --all-targets --all-features -- -D warnings`
   - `cargo test --all`
5. Open a Pull Request with a clear description.

## What to Contribute

- Bug fixes
- Performance improvements
- New plugins/systems
- Protocol/network improvements
- Documentation and examples

### Good First Issues
If you are new to the project or to Rust, we highly encourage you to search the Issue tracker for the label **`good first issue`**. These are targeted tasks designed to be approachable and an excellent way to get familiar with our architecture. Don't hesitate to ask questions on the issues—we are happy to guide you!

If a change is large, open an issue first to align on design.

## Project Principles

- Keep the core minimal.
- Prefer compile-time composition over runtime complexity.
- Avoid unnecessary allocations and hidden overhead.
- Write clear, testable, modular code.
- Preserve plugin ergonomics for framework users.

## Coding Guidelines

- Use idiomatic Rust.
- Keep modules and functions small and focused.
- Add tests for behavior changes.
- Update docs/examples when APIs change.
- Avoid unrelated refactors in the same PR.

## Commit and PR Expectations

- Use descriptive commit messages.
- PR title: short and specific.
- PR description should include:
  - What changed
  - Why it changed
  - How it was tested
  - Any breaking changes

Small, reviewable PRs are preferred.

## Reporting Bugs

Please include:

- Expected behavior
- Actual behavior
- Steps to reproduce
- Rust version (`rustc --version`)
- Platform/OS
- Relevant logs or stack traces

## Security

Do not publish security issues publicly first.
Report them privately to the maintainers.

## License

By contributing, you agree that your contributions are licensed under the MIT License in `LICENSE.md`.
