# Requirement Checklist

| School requirement | Repo evidence | Status |
| --- | --- | --- |
| Define technical performance indicators | KPI table in [Performance Measurement](./index.md) | Met |
| Measure performance with reproducible tools | Criterion commands in [Benchmarking & Profiling](./benchmarking.md), TCP POC under `pocs/performance/` | Met |
| Integrate tests into the development cycle | Correctness commands, CI coverage workflow, and manual benchmark commands | Mostly met |
| Run comparative performance tests | Codec comparison and channel handoff comparison in [Metrics & Results](./metrics-results.md) | Met |
| Analyze bottlenecks | Chunk packet benchmark identifies chunk serialization as a heavier path; optimization log records channel, packet ingest, chunk generation, and codec tradeoffs | Met |
| Optimize based on measurements | Packet ingest budget, chunk generation budget, slow tick warning, `flume` architecture boundary, and custom codec are documented in [Optimization Log](./optimization-log.md) | Met |
| Document results for reviewers | Dated result tables in [Metrics & Results](./metrics-results.md) | Met |
| Stress or resilience testing | TCP connect stress POC with a 64-client local sample | Partially met |
| Memory monitoring | `/usr/bin/time -v` command documented | Partially met |

## Remaining Improvements

The objective is defense-ready, but two improvements would make the evidence stronger:

1. Add a full Minecraft handshake/login stress tool once protocol support is mature enough.
2. Record a dated memory table for idle, connection burst, and chunk-send scenarios.
