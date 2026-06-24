# Architecture Objective Checklist

This page maps the school objective "Structurer, documenter et fiabiliser l'architecture technique du projet" to concrete VoidMC evidence.

| Requirement | Evidence | Status |
|---|---|---|
| Present project architecture | [Architecture Overview](/architecture/), [Architecture Reference](/reference/server/architecture) | Covered |
| Present components/services/dependencies | Workspace table in `README.md`, [Architecture Overview](/architecture/) | Covered |
| Present data flow | Packet flow in [Architecture Reference](/reference/server/architecture), runtime diagram in [Deployment](./deployment) | Covered |
| Present deployment | [Deployment](./deployment), release run commands in `README.md` | Covered |
| Present versioning | `Cargo.toml`, `Cargo.lock`, `release-plz.toml`, `CHANGELOG.md` | Covered |
| Justify structural choices | [Technology Evaluations](/architecture/technology-evaluations), [Tech Watch](/architecture/tech-watch/) | Covered |
| Complete README | Root `README.md` | Covered |
| Advanced documentation | Architecture diagrams, protocol reference, ECS reference, operational-readiness docs | Covered |
| Code standards | [Quality Standards](/architecture/quality-standards), CI workflow | Covered |
| Static analysis tooling | Clippy with `-D warnings`, Rust compiler checks | Covered |
| Tests on critical modules | `void-codec`, `void-protocol`, `void-data`, command/parser/world tests | Covered |
| Error handling and logs | Typed decode errors, structured `tracing`, example file logs | Covered |
| Security minimum | [Security and Reliability](./security-reliability) documents current protections and limits | Covered with limitations |
| Configuration example | [Configuration Examples](./configuration-example), [Server Configuration](/reference/server/configuration) | Covered |
| Deployment automation script | Not included in this pass; release Cargo commands are documented | Current limitation |

## Defense Path

For a short review, present the evidence in this order:

1. Root `README.md` for project overview and commands.
2. [Architecture Overview](/architecture/) for system shape.
3. [Architecture Reference](/reference/server/architecture) for packet and thread flow.
4. [Quality Standards](/architecture/quality-standards) for CI, linting, and tests.
5. [Deployment](./deployment) and [Security and Reliability](./security-reliability) for operational readiness.
6. This checklist to map the subject requirements to artifacts.

