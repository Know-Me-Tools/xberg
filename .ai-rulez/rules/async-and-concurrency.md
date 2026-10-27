---
type: Decision
title: Async And Concurrency
description: "Xberg async rules: tokio-only extraction paths, spawn_blocking for CPU work, Send + Sync types, timeouts. Use when editing core async code."
paths:
  - "crates/xberg/src/**"
x-ai-rulez:
  kind: rule
  id: async-and-concurrency
  metadata:
    priority: high
---

- Extraction paths are fully async on tokio.
- Do not block the async runtime; use `spawn_blocking` for CPU-intensive work.
- Public types are `Send + Sync`.
- Use `tokio::time::timeout` for extraction timeouts. `tokio::select!` is used only in the MCP server and API startup
  paths, never on an extraction path.
- Cross-platform: test on Linux (amd64, arm64) and macOS at minimum.
