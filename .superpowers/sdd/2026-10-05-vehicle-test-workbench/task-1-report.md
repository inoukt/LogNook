# Task 1 implementation report — typed research-test contract

## Implemented

- Added internal Rust `ResearchTestRequest` variants for backend-reloaded OEM candidate specs and fixed A30 read tests.
- Added serializable `ResearchTestResult` and `ResearchTestSelection` enums using the agreed externally tagged `type` / `payload` camelCase envelope. Selection carries the catalog candidate only as report metadata.
- Added serializable `ResearchRunItem` and `ResearchRunReport` report types.
- Added `HardwareAdapter::execute_research_test`, whose default returns a clear read-only research unsupported error.
- Added tests for default rejection and tagged result serialization/deserialization.

## TDD and verification

- RED: `cargo test -p lognook-core research::` failed to compile because `ResearchTestRequest`, `ResearchTestResult`, and `execute_research_test` were missing.
- GREEN: `cargo test -p lognook-core research::` passed: 2 passed, 0 failed.
- Full core suite: `cargo test -p lognook-core` passed: 104 unit tests and all integration tests passed; doc tests passed (0 tests).
- Formatting: `rustfmt --edition 2024 --check crates/lognook-core/src/research.rs crates/lognook-core/src/adapter.rs` passed. `cargo fmt --all -- --check` reports formatting differences in existing unrelated workspace files; those files were left unchanged.
- `git diff --check` passed.

## Scope and limitations

No transport implementation, acquisition scheduling, UI command, or frontend candidate-spec input was added; these are outside Task 1. Full workspace formatting remains non-clean due to existing unrelated diffs reported by rustfmt.

## Next task

Task 2 should implement the adapter-specific read-only transport support using this typed request/result contract.