#!/bin/bash
set -euo pipefail
# guard-registry:policy.hexagonal.current-layer-matrix
# guard-registry:policy.task.target-layout
# guard-registry:migration.runtime.application-accessors-to-adapters
# guard-registry:migration.runtime.application-from-args-to-adapters
# guard-registry:migration.runtime.legacy-port-to-application

# 功能：检查未迁移 feature 的 COLA 分层，并锁定已迁移 feature 的目标目录。
# 作用：普通 feature 继续受迁移期 COLA 依赖方向约束；Runtime 使用
#       domain/application/ports/adapters/shared；Workflow 使用 domain；Storage 使用 domain/ports/adapters；
#       Project/Tools/Task 使用 domain/adapters（domain 不得依赖 adapters）；Audit 仅允许随真实 Usage 交付增量建立的 Hexagonal 层。
# 例外：少量已登记的迁移期层级倒置（见 xtask cola_layer_purity 的 narrow migration exceptions 列表）。
#
# 实现：Rust xtask 子命令 `cola-layer-purity`（tools/xtask/src/cola_layer_purity.rs）。
# 历史：python heredoc 实现在部分环境 stdin 卡死导致 push 阻断（#1500），已整体移植为 Rust。

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$ROOT"

cargo run --quiet -p xtask -- cola-layer-purity "$ROOT"
