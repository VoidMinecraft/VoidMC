# Sharing and Openness

This page records how technology-watch findings are shared so they do not remain private notes.

## Internal Sharing

| Artifact | Audience | Purpose |
|---|---|---|
| `AGENTS.md` | Developers and AI coding agents | Share architecture rules, runbook commands, and safe change patterns. |
| `void-docs/docs/architecture/technology-evaluations.md` | Developers, reviewers, mentors | Explain why major technologies were accepted or rejected. |
| `docs/protocol-diff-1.21.4-to-26.1.2.md` | Protocol implementers | Share upstream protocol findings before implementation. |
| `CHANGELOG.md` | Team and users | Record visible technical changes and feature evolution. |
| Benchmark docs | Team and reviewers | Share measured evidence for technical choices. |

## External Sharing Targets

These channels are relevant when asking for feedback or validating assumptions:

| Channel | Topic to share |
|---|---|
| GitHub issues/discussions | Protocol compatibility questions, benchmark findings, plugin API feedback |
| Rust community channels | Async runtime, ECS, macro, and ownership questions |
| Minecraft protocol communities | Packet layout, version migration, registry format questions |
| Project README/docs | Public summaries of accepted decisions and reproducible commands |

## Evidence Policy

When a team member receives useful feedback from Discord, Reddit, StackOverflow, GitHub, or another technical community, record:

- Date and channel.
- Question or topic.
- Link or screenshot location.
- Decision or code/doc change caused by the feedback.

Keep these records focused on technical learning. Open-source contribution strategy is tracked separately from this technology-watch objective.

