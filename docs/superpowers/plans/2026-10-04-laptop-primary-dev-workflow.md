# Laptop-Primary Development Workflow Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the laptop the reliable build and physical-test host while this desktop contributes code through GitHub.

**Architecture:** Keep independent local clones on both machines and use GitHub `main` as the tested handoff point. The laptop runs LogNook beside the USB adapter; this desktop develops task branches and pushes them for review and merge.

**Tech Stack:** Git/GitHub, Rust/Cargo, Tauri 2, Node/npm, the 32-bit Windows J2534 worker, existing OpenPort 2 Mode 01 adapter.

**Spec:** `docs/superpowers/specs/2026-10-04-laptop-primary-dev-workflow-design.md`

## Global Constraints

- Keep the USB adapter, Windows driver, and J2534 provider on the laptop during physical tests.
- Use only LogNook's existing read-only Mode 01 OpenPort path for this physical smoke test.
- Keep source sync in Git; do not copy or cloud-sync one checkout over another.
- Test only commits already merged to `main`; write down the commit SHA before each vehicle run.
- Never discard dirty laptop changes with reset/clean commands; stop and preserve them before syncing.
- Do not perform a Git pull/build while acquisition or recording is active.
- Do not change adapter drivers or automatically switch J2534 providers.

## Review Focus

- Laptop has local edits when `main` advances: stop sync, save work in a commit or separate branch, and preserve exact files; never overwrite.
- `origin/main` changes during a vehicle session: keep testing the checked-out SHA and pull only after disconnect/recording close.
- Node exists but npm is missing, or x86 MSVC target/linker is absent: report the exact missing tool before attempting the hardware session.
- Provider identity or registry entry is absent: record the result and stop before connecting to the ECU.
- Network drops between desktop and GitHub while coding: finish local work in the task branch and sync it later; do not create competing folder copies.

---

### Task 1: Check laptop readiness and synchronize the tested source

**Files:**
- Read: `docs/superpowers/specs/2026-10-04-laptop-primary-dev-workflow-design.md`
- Read: `docs/hardware/2026-10-02-openport2.md`

**Interfaces:**
- Consumes: GitHub `origin/main` and the laptop's local checkout.
- Produces: a clean laptop checkout at a recorded commit SHA, plus a written list of missing prerequisites.

- [ ] **Step 1: Inspect the checkout before syncing**

Run from the laptop repository root: `git status --short --branch` and `git rev-parse --short HEAD`. If files are modified or untracked, preserve them on a branch or leave them untouched and resolve them before pulling.

- [ ] **Step 2: Fast-forward to GitHub main**

Run `git fetch origin` then `git pull --ff-only origin main`. Expected: no merge commit and no overwritten local files. Record the resulting SHA.

- [ ] **Step 3: Record toolchain availability**

Run `git --version`, `rustc --version`, `cargo --version`, `rustup target list --installed`, `node --version`, and `npm --version`. Confirm the x86 MSVC linker and Windows SDK are installed. If any are missing, document the missing prerequisite before continuing.

### Task 2: Build and smoke-test LogNook on the laptop

**Files:**
- Read: `apps/desktop/package.json`
- Read: `apps/desktop/src-tauri/tauri.conf.json`

**Interfaces:**
- Consumes: clean laptop checkout and Rust/Node/MSVC prerequisites from Task 1.
- Produces: a running development build and successful simulator samples on the laptop.

- [ ] **Step 1: Prepare dependencies**

From `apps/desktop`, run `npm ci`. Once per machine, run `rustup target add i686-pc-windows-msvc`.

- [ ] **Step 2: Run software checks**

From the repository root, run `cargo test --workspace -j 1`. From `apps/desktop`, run `npm run test:ui` and `npm run build`. Expected: all commands pass. If the default Cargo target drive lacks space, use another local drive for `CARGO_TARGET_DIR` and record the path.

- [ ] **Step 3: Build and start the app**

From `apps/desktop`, run `npm run build:j2534-worker` followed by `npm run tauri dev`. Expected: the LogNook window opens and can connect to the simulator and display samples.

### Task 3: Use a Git handoff for desktop development work

**Files:**
- Modify: source/docs files required by the chosen coding task.
- Test: the tests covering those files, plus the Task 2 checks when hardware-facing Rust, Tauri commands, or the UI changes.

**Interfaces:**
- Consumes: current `origin/main` and one scoped user/developer task.
- Produces: a pushed `codex/<task>` branch with tests and a reviewable commit; only merged `main` is handed to the laptop for physical testing.

- [ ] **Step 1: Start a task branch from current main**

Run `git fetch origin`, create `codex/<task>` from `origin/main`, and confirm `git status` is clean before editing.

- [ ] **Step 2: Implement and verify the scoped task**

Run the task's focused tests. For adapter, acquisition, Tauri, or UI changes, also run `cargo test --workspace -j 1`, `npm run test:ui`, `npm run build`, and `npm run build:j2534-worker` as applicable.

- [ ] **Step 3: Push the branch for integration**

Commit the implementation and verification results, then run `git push -u origin codex/<task>`. Merge it into `main` only after review and a passing test result.

### Task 4: Bring merged work to the laptop and run a bounded adapter test

**Files:**
- Modify: `docs/hardware/2026-10-02-openport2.md` with observed results only.
- Modify: `PROJECT_STATUS.md` to mark physical validation complete or record the remaining blocker.

**Interfaces:**
- Consumes: a merged `origin/main` commit and the laptop's installed OpenPort provider.
- Produces: a hardware-test record tied to a specific commit SHA; no compatibility claim without successful ECU replies.

- [ ] **Step 1: Sync only after stopping acquisition**

Confirm the app is disconnected, recording is closed, and `git status` is clean. Run `git pull --ff-only origin main`, then record `git rev-parse --short HEAD`.

- [ ] **Step 2: Build the helper and run LogNook**

From `apps/desktop`, run `npm run build:j2534-worker`, then `npm run tauri dev`. Select the intended registered provider, identify it, and connect only after identity succeeds.

- [ ] **Step 3: Verify common-pipeline measurements**

Use the 2014 Scion xB and only PIDs reported supported. Confirm live values update, then make a short recording and verify replay/CSV export if acquisition remains stable. Do not issue service 21/22 requests through this OpenPort build.

- [ ] **Step 4: Save evidence and disconnect**

Record commit SHA, provider identity, API/DLL/firmware, ECU response IDs, supported PIDs, raw response evidence, latency/sample rate, errors, and recording result in `docs/hardware/2026-10-02-openport2.md`. Disconnect and stop recording cleanly.

## Final Verification

- Laptop `git status --short --branch` is clean after each merge and build.
- Simulator smoke test succeeds before physical testing.
- The development app and x86 worker both launch from the laptop checkout.
- Physical support is reported only for PIDs that returned valid ECU responses; a provider identity alone is not vehicle validation.
- `PROJECT_STATUS.md` states the tested commit and the exact remaining physical limitation.
