# Contributing to Void

Thank you for your interest in contributing to Void! We welcome contributions from developers of all skill levels. This guide will help you get started.

## Our Values

- **Quality**: We prioritize code quality and thorough testing
- **Collaboration**: We value thoughtful discussion and diverse perspectives
- **Documentation**: Code should be clear and well-documented
- **Community**: We're building something together

## Code of Conduct

### Our Pledge

We are committed to providing a welcoming and inclusive environment for all contributors, regardless of age, body size, disability, ethnicity, gender identity, experience level, nationality, personal appearance, political beliefs, religion, or sexual identity and orientation.

### Expected Behavior

- Use welcoming and inclusive language
- Be respectful of differing opinions and experiences
- Gracefully accept constructive criticism
- Focus on what is best for the community

### Unacceptable Behavior

- Harassment, intimidation, or discrimination of any kind
- Offensive comments related to personal characteristics
- Deliberate intimidation or threats
- Unwanted sexual attention or advances

### Reporting

If you experience or witness unacceptable behavior, please report it by contacting the project maintainers privately.

## Getting Started

### Fork & Clone

```bash
# Fork on GitHub, then clone your fork
git clone https://github.com/YOUR-USERNAME/void.git
cd void

# Add upstream remote for syncing
git remote add upstream https://github.com/void-minecraft/void.git
```

### Setup Development Environment

```bash
# Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Build the project
cargo build

# Run tests
cargo test

# Check code style
cargo fmt --check
cargo clippy

# Generate documentation
cargo doc --no-deps --open
```

## Development Workflow

### 1. Create a Feature Branch

```bash
# Sync with upstream
git fetch upstream
git checkout upstream/main

# Create feature branch
git checkout -b feature/your-feature-name
```

**Naming conventions:**

- Features: `feature/short-description`
- Bugfixes: `fix/issue-number-description`
- Documentation: `docs/what-you-improved`

### 2. Make Changes

**Code style:**

- Follow Rust conventions (rustfmt)
- Keep lines reasonably short (~100 characters)
- Use meaningful variable and function names
- Add comments for complex logic

**Example:**

```rust
/// Represents a player in the game world.
///
/// This struct is shared across async tasks and must be kept thread-safe.
pub struct Player {
    pub id: u32,
    pub name: String,
    pub position: Position,
}

impl Player {
    /// Creates a new player with the given name.
    pub fn new(id: u32, name: String) -> Self {
        Self {
            id,
            name,
            position: Position::default(),
        }
    }
}
```

### 3. Commit Changes

```bash
git add .
git commit -m "type(scope): description"
```

**Commit message format:**

```
type(scope): description

Optional longer explanation of the change, why it was made,
and any relevant context.

Closes #123  (if fixing an issue)
```

**Types:**

- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation update
- `test`: Test addition or modification
- `refactor`: Code restructuring without behavior change
- `perf`: Performance improvement
- `style`: Code style changes (formatting, etc.)

### 4. Test Your Changes

```bash
# Run all tests
cargo test

# Run specific test
cargo test test_name

# Run tests with output
cargo test -- --nocapture

# Test with coverage (if configured)
cargo tarpaulin --out Html
```

**Add tests for:**

- New features
- Bug fixes
- Edge cases
- Error conditions

Example test:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_player_creation() {
        let player = Player::new(1, "Steve".to_string());
        assert_eq!(player.id, 1);
        assert_eq!(player.name, "Steve");
    }
}
```

### 5. Format & Lint

```bash
# Format code
cargo fmt

# Check for issues
cargo clippy -- -D warnings

# Fix some clippy warnings automatically
cargo clippy --fix
```

### 6. Push & Create Pull Request

```bash
# Push to your fork
git push origin feature/your-feature-name

# Open PR on GitHub
# Link related issues with "Closes #123"
# Provide a clear description of changes
```

## Pull Request Guidelines

### PR Title

- Be clear and descriptive
- Include the type prefix: `feat:`, `fix:`, `docs:`, etc.
- Example: `feat(protocol): add support for Play state packets`

### PR Description

- **What**: Brief summary of the change
- **Why**: Motivation and context
- **How**: Technical approach (if complex)
- **Testing**: How you tested it
- **Related Issues**: Link any related issues

### PR Checklist

Before submitting, ensure:

- [ ] Code follows project style (`cargo fmt`)
- [ ] No clippy warnings (`cargo clippy`)
- [ ] Tests pass (`cargo test`)
- [ ] New tests added for new features
- [ ] Documentation updated or added
- [ ] Commit messages are clear and well-formatted
- [ ] Branch is up-to-date with main

## Areas for Contribution

### High Priority

1. **Packet Implementation**

   - Add missing packet types for any game state
   - Improve packet ID mappings
   - Add field validation

2. **Performance Optimization**

   - Reduce memory allocations
   - Optimize packet serialization
   - Improve async task management

3. **Testing**

   - Add integration tests
   - Add unit tests for codec
   - Create stress tests

4. **Documentation**
   - Improve architecture docs
   - Add inline code comments
   - Create tutorials

### Good First Issues

New to the project? Look for issues labeled:

- `good-first-issue`: Perfect for beginners
- `help-wanted`: Need community help
- `documentation`: Doc improvements

## Review Process

### Reviewer Responsibilities

- Check code quality and style
- Verify tests are adequate
- Look for performance issues
- Suggest improvements

### Author Responsibilities

- Respond to feedback promptly
- Make requested changes
- Ask questions if unclear
- Be open to criticism

### Merge Criteria

PRs are merged when:

- All tests pass
- Code review is approved
- Documentation is complete
- No conflicts with main branch

## Community Channels

### Get Help

- **GitHub Issues**: For bug reports and feature requests
- **GitHub Discussions**: For questions and ideas
- **Discord**: Join our community server (link in README)
- **Email**: Direct contact with maintainers

### Stay Updated

- Watch the repository for updates
- Follow releases on GitHub
- Subscribe to changelog
- Check out discussions

## Licensing

By contributing, you agree that your contributions will be licensed under the project's license (see LICENSE file).

## Questions?

Don't hesitate to ask! The best way to learn is by contributing.

- Open a GitHub Discussion
- Leave a comment on an issue
- Reach out to maintainers directly

**Thank you for being part of Void! 🎮**
