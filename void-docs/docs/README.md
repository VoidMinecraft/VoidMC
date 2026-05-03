# Void Documentation

Welcome to the Void documentation site. This is the central resource for understanding, developing, and contributing to the Void Minecraft server.

## What You'll Find Here

### 📚 [Getting Started](/guide/getting-started)

Quick setup instructions and an introduction to Void's core concepts.

### 🏗️ [Architecture](/architecture)

Deep dive into Void's modular design, protocol state machine, and component interactions.

### 🔧 [Development Guide](/guide/development)

Setup your development environment, learn our coding standards, and contribution workflow.

### ✅ [Testing Guide](/guide/testing)

How to write tests, run the test suite, and ensure code quality.

### 🤝 [Contributing](/contributing)

Guidelines for contributing to Void, code review process, and community standards.

### 📖 [Code of Conduct](/contributing/code-of-conduct)

Our commitment to a welcoming and inclusive community.

### ⚖️ [License & IP Protection](/contributing/license)

Information about Void's MIT license and IP ownership.

### 📊 [Performance Metrics](/performance)

How we measure and optimize Void's performance, with testing strategies.

### 📈 [Benchmarking & Profiling](/performance/benchmarking)

Tools and techniques for identifying and fixing performance bottlenecks.

## Quick Links

- **GitHub Repository**: [VoidMinecraft/VoidMC](https://github.com/VoidMinecraft/VoidMC)
- **Issues & Discussions**: [GitHub Issues](https://github.com/VoidMinecraft/VoidMC/issues)
- **Community**: See README for Discord/community channels

## For Different Roles

### 👨‍💻 Developers

1. Start with [Getting Started](/guide/getting-started)
2. Read [Development Guide](/guide/development)
3. Check [Architecture](/architecture) for deep understanding
4. Review [Contributing](/contributing) guidelines

### 🧪 QA / Performance Testers

1. Read [Performance Metrics](/performance)
2. Learn [Benchmarking & Profiling](/performance/benchmarking)
3. Review [Testing Guide](/guide/testing)

### 📝 Documentation Contributors

1. Check [Contributing](/contributing) for process
2. Review existing documentation
3. Follow [Code of Conduct](/contributing/code-of-conduct)

### 🎓 Students / Researchers

1. Start with [Architecture](/architecture)
2. Explore [Performance Metrics](/performance)
3. Review [Benchmarking](/performance/benchmarking)
4. Consider [Contributing](/contributing)

## EIP Project Objectives

This documentation supports our official EIP track objectives:

### ✅ Mandatory Objectives

1. **Evaluate and integrate new technologies** (veille technologique)
2. **Structure and document architecture** - _This site!_

### ✅ Selected Secondary Objectives

- **B: Community Contributions** - See [Contributing](/contributing)
- **D: Performance Testing & Optimization** - See [Performance](/performance)

## Navigation Tips

- Use the search feature (top of page) to find topics quickly
- Click on the section headers to expand/collapse topics
- Each page has links to related content
- Code examples are copy-friendly

## Contributing to Documentation

Found a typo? Want to improve a section? See [Contributing](/contributing) for how to submit documentation improvements.

## Development Commands

```bash
bun install --frozen-lockfile

# Start development server
bun run dev

# Build static site
bun run build

# Preview build
bun run preview

# Format & check code (requires `biome` in devDependencies)
bun run format
bun run check
```

Visit `http://localhost:5173` after `bun run dev`

---

**Last Updated**: May 3, 2026
