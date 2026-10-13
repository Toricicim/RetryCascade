# Changelog

## 0.2.0 — 2026-10-13

- Preserved V1 deterministic simulation and default legacy V2 behavior.
- Added V2 pre-admission failures, no-retry controls, request-stable demand
  and run-global budget comparisons.
- Added cohort success/failure latency, work/busy ledgers and explicit
  timeout/budget client-contract sensitivity.
- Added calendar-aligned traces, causal descriptive recovery features,
  separate incident/early-post outcomes and analytical IID references.
- Added frozen plans, binary/source snapshots, resume, failure records,
  checksum audits and exact replay on the same binary/platform.
- Added DuckDB/Parquet analysis, SQL examples and deterministic figure reports.
- Published a verified, curated 630-run exploratory follow-up snapshot;
  raw runs, private docs and host paths remain outside Git.
- Added a public snapshot exporter and GitHub Actions verification.
- Removed the unused empty Docker Compose placeholder; no external service
  is required to use this release.

Package release numbers are separate from config/model/summary schema versions.
Real-system validation, correlated failure models, cancellation, universal
database adapters and LLM interfaces are not claimed as completed features.

## 0.1.0 — deterministic core

Resolved strict config, event scheduler, bounded FIFO pipeline, Sink capacity
loss, lifecycle ledgers, four retry policies and recovery metrics.
