# Issue #972 Code Organization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Establish a capability-first, complexity-aligned code organization standard for aemeath, remove the legacy fixed-layer template from target design semantics, and preserve traceability to mature static-language ecosystems.

**Architecture:** `docs/design/01-system/06-code-organization.md` becomes the system-level source of truth. Existing system, module, guard, and migration documents are aligned around dependency direction, narrow public façades, use-case colocation, and ports introduced only at real external seams. The legacy fixed-layer guard remains an explicitly tracked migration item; this documentation PR does not rewrite its implementation.

**Tech Stack:** Markdown design documents, Rust module visibility and Cargo workspace concepts, GitHub Issues/PRs, repository architecture guard scripts.

---

### Task 1: Add the system-level code organization standard

**Files:**
- Create: `docs/design/01-system/06-code-organization.md`

- [x] **Step 1: Write the decision and non-goals**

  State the adopted shape as `capability-first modular monolith + use-case colocation + ports on demand`. Explicitly distinguish DDD strategic design, Hexagonal inside/outside isolation, Clean dependency direction, and Vertical Slice change locality from directory templates.

- [x] **Step 2: Define the progressive structure ladder**

  Document the promotion sequence:

  ```text
  flat capability module
    -> cohesive use-case/capability submodules
    -> optional shared model for cross-use-case invariants
    -> optional ports for real external seams
    -> optional crate boundary for compiler isolation or stable published language
  ```

  Include explicit entry and exit criteria for `model/`, ports, technology-specific directories, and new crates.

- [x] **Step 3: Add aemeath examples**

  Include three target examples: a small policy module, a provider integration module named by protocol/provider, and the complex Runtime module organized around run/loop/coordinator capabilities.

- [x] **Step 4: Add ecosystem and paradigm examples**

  Cover JVM/Spring Modulith, .NET eShop and Vertical Slice, Go official module layout, Rust rust-analyzer/Helix, and Chromium components. Each example must show a compact tree, the boundary mechanism, and the part aemeath does or does not copy.

- [x] **Step 5: Add decision traceability**

  End with a table whose columns are `最终决策`, `主要参考`, `借鉴`, and `未照搬`. Link primary sources close to each claim and include rejected alternatives with reasons.

### Task 2: Align system design and navigation

**Files:**
- Modify: `docs/design/README.md`
- Modify: `docs/design/01-system/04-system-architecture.md`
- Modify: `docs/design/01-system/05-dependency-rules.md`

- [x] **Step 1: Register the new source of truth**

  Add `06-code-organization.md` to the system navigation and reading paths. Describe `05-dependency-rules.md` without naming the retired directory template.

- [x] **Step 2: Rewrite the architecture decision**

  Keep DDD-guided modular boundaries, selective Hexagonal seams, Clean dependency direction, and the single Composition Root. Remove wording that implies every module has mandatory domain/application/adapter layers, and link the new organization standard.

- [x] **Step 3: Rewrite dependency rules around policy and detail**

  Replace the fixed three-layer diagram with `external detail -> capability policy`, clarify that ports are declared by the consumer at actual volatile boundaries, and replace the retired-template section with a rule that directory names do not prove dependency direction.

- [x] **Step 4: Update related-document links and modification histories**

  Add Issue #972 links and ensure each changed target document keeps its required related-documents and history sections.

### Task 3: Align module and engineering governance documents

**Files:**
- Modify: `docs/design/02-modules/project/02-ports-and-adapters.md`
- Modify: `docs/design/03-engineering/architecture-guards.md`
- Modify: `docs/design/03-engineering/migration-governance.md`
- Modify: `AGENTS.md`
- Modify: `specs/project.md`

- [x] **Step 1: Make the Project target tree capability-first**

  Remove the Current-vs-Target fixed-layer comparison from the target-only module document. Keep only the target `workspace/`, `git/`, and narrow façade structure, and refer to migration governance for current paths.

- [x] **Step 2: Reclassify the legacy fixed-layer guard**

  In the guard registry, stop presenting the legacy guard as a target architecture principle. Keep its exact script identifier for runtime truth, describe it as a temporary migration guard, and point its replacement criteria to the new code organization standard.

- [x] **Step 3: Track the guard and directory migration explicitly**

  Add a Current-to-Target entry to `migration-governance.md`: current feature directories and the fixed-layer guard remain until a dedicated Guard issue replaces them with public-surface, cross-feature, cycle, and Composition Root checks.

- [x] **Step 4: Remove the retired concept from project instructions**

  Update the Project row in `AGENTS.md` to describe workspace ownership and the git outbound port without naming or mandating the retired directory template. Reframe the existing directory constraints in `specs/project.md` as migration-period implementation constraints, preserving the runtime truth without retaining the retired architecture concept.

### Task 3.5: Resolve repository-wide active-document contradictions

**Files:**
- Modify: `README.md`
- Modify: `specs/update.md`
- Modify: `docs/design/01-system/04-system-architecture.md`
- Modify: `docs/design/02-modules/project/README.md`
- Modify: `docs/design/02-modules/project/01-domain-model.md`
- Modify: `docs/design/03-engineering/architecture-guards.md`
- Modify: `docs/design/03-engineering/migration-governance.md`

- [ ] **Step 1: Align repository entry points and active instructions**

  Replace stale root navigation with the three-level design index and code-organization source of truth. Reframe Update's current flat files as a migration-period implementation constraint, without treating the retired concept as architecture or hard-coding the guard count.

- [ ] **Step 2: Eliminate duplicate Project target decisions**

  Make `02-ports-and-adapters.md` the only Project physical-layout and port-contract source. Remove the duplicate tree and optional broad super-trait from the Project README, require Composition Root injection of `GitWorktreeOps`, and track the current self-construction gap in Migration Governance.

- [ ] **Step 3: Reconcile system-level criteria and guard runtime wording**

  Delegate crate promotion criteria to `06-code-organization.md` §3.6, keep DDD strategic identification separate from optional tactical aggregates, and describe the legacy guard's path-based test exclusion exactly.

- [ ] **Step 4: Re-run independent specification and quality reviews**

  Resolve all Critical and Important findings before repository verification.

### Task 4: Validate the documentation set

**Files:**
- Verify all files changed in Tasks 1-3.5.

- [ ] **Step 1: Check the retired concept is absent from target design prose**

  Run:

  ```bash
  rg -n -i '\bCOLA\b' README.md AGENTS.md specs docs/design
  ```

  Expected: the only permitted match is the exact legacy script identifier in `architecture-guards.md` and `migration-governance.md`; no target architecture prose uses the concept.

- [ ] **Step 2: Check required coverage and source traceability**

  Run:

  ```bash
  rg -n 'Spring Modulith|eShop|Vertical Slice|Go module|rust-analyzer|Helix|Chromium|最终决策|未照搬' docs/design/01-system/06-code-organization.md
  ```

  Expected: every ecosystem and the decision traceability section is present.

- [ ] **Step 3: Check Markdown whitespace and links mechanically**

  Run:

  ```bash
  git diff --check
  python3 - <<'PY'
  from pathlib import Path
  import re

  root = Path('docs/design')
  failures = []
  for path in root.rglob('*.md'):
      text = path.read_text()
      for target in re.findall(r'\[[^]]+\]\(([^)]+)\)', text):
          if '://' in target or target.startswith('#'):
              continue
          target = target.split('#', 1)[0]
          if target and not (path.parent / target).resolve().exists():
              failures.append(f'{path}: missing {target}')
  if failures:
      raise SystemExit('\n'.join(failures))
  print('relative markdown links: OK')
  PY
  ```

  Expected: no whitespace errors and no missing relative Markdown targets.

  Result: all 10 changed Markdown files pass. The full-tree scan still reports the pre-existing Server Foundation link missing from `docs/design/02-modules/server/01-design.md`; the same link is present on `origin/release/v0.1.0` and is outside #972.

- [ ] **Step 4: Run architecture and repository verification**

  Run:

  ```bash
  .agents/hooks/check-architecture-guards.sh
  cargo test --workspace
  ```

  Expected: both commands exit 0.

  Result: architecture guards pass with Homebrew Python on `PATH` because the existing script uses Python 3.10+ union syntax while macOS `/usr/bin/python3` is 3.9.6; `cargo test --workspace` and `cargo clippy --workspace --all-targets` pass.

### Task 5: Integrate through the release branch

**Files:**
- Modify only if review findings require corrections.

- [ ] **Step 1: Request an independent review**

  Review the branch diff against Issue #972, focusing on contradictory target rules, inaccurate ecosystem examples, missing decision provenance, and guard/document drift. Resolve all Critical and Important findings.

- [ ] **Step 2: Commit the completed documentation**

  Run:

  ```bash
  git add docs/design docs/superpowers/plans/2026-07-14-issue-972-code-organization.md AGENTS.md specs/project.md
  git commit -m "docs(architecture): define adaptive code organization (#972)"
  ```

- [ ] **Step 3: Synchronize with the integration branch**

  Run:

  ```bash
  git pull origin release/v0.1.0
  ```

  If it changes the branch, rerun Task 4 in full.

- [ ] **Step 4: Push and create the PR**

  Push `docs/972-code-organization` and create a PR with base `release/v0.1.0`. The PR body must reference Issue #972, state that the legacy guard rewrite is out of scope, and list the exact verification commands.
