#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SOURCE_ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$SCRIPT_DIR/../.." && pwd)}"
if [ -n "${AEMEATH_PROJECT_DIR:-}" ] && [ ! -d "${AEMEATH_PROJECT_DIR}/.agents/hooks" ]; then
  SOURCE_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
fi
GUARD="$SOURCE_ROOT/.agents/hooks/check-tui-retained-output-view.sh"

TMP_ROOT="$(mktemp -d)"
trap 'rm -rf "$TMP_ROOT"' EXIT
mkdir -p "$TMP_ROOT/.agents/hooks" \
  "$TMP_ROOT/apps/cli/src/tui/app" \
  "$TMP_ROOT/apps/cli/src/tui/view_assembler" \
  "$TMP_ROOT/apps/cli/src/tui/model/conversation" \
  "$TMP_ROOT/apps/cli/src/tui/render/output"
cp "$GUARD" "$TMP_ROOT/.agents/hooks/"

write_valid_fixture() {
  cat >"$TMP_ROOT/apps/cli/src/tui/app/update.rs" <<'EOF'
let result = self.output_view.retained.materialize_window(
    &self.model.conversation,
    workspace_root,
    request,
);
EOF
  cat >"$TMP_ROOT/apps/cli/src/tui/view_assembler/output.rs" <<'EOF'
pub(super) fn assemble_item() {}
EOF
  cat >"$TMP_ROOT/apps/cli/src/tui/view_assembler/retained_output_view.rs" <<'EOF'
pub struct RetainedOutputView {
    window_index: OutputWindowIndex,
}
EOF
  cat >"$TMP_ROOT/apps/cli/src/tui/model/conversation/output_view_change.rs" <<'EOF'
const OUTPUT_VIEW_JOURNAL_CAPACITY: usize = 256;
fn bound(entries: &mut std::collections::VecDeque<()>) { entries.pop_front(); }
EOF
  cat >"$TMP_ROOT/apps/cli/src/tui/render/output/document_renderer.rs" <<'EOF'
fn render_materialized_window(view_model: &OutputViewModel) {
    for root in &view_model.roots {
        render_root(root);
    }
}
EOF
}

write_valid_fixture
AEMEATH_PROJECT_DIR="$TMP_ROOT" "$TMP_ROOT/.agents/hooks/check-tui-retained-output-view.sh"

write_valid_fixture
cat >"$TMP_ROOT/apps/cli/src/tui/view_assembler/retained_output_view.rs" <<'EOF'
pub struct RetainedOutputView {
    view_model: OutputViewModel,
}
EOF
if AEMEATH_PROJECT_DIR="$TMP_ROOT" "$TMP_ROOT/.agents/hooks/check-tui-retained-output-view.sh" >/dev/null 2>&1; then
  echo "guard should reject retained full output view model" >&2
  exit 1
fi

write_valid_fixture
printf '%s\n' 'fn rebuild() { OutputViewAssembler::assemble_shared_roots(conversation, None); }' \
  >>"$TMP_ROOT/apps/cli/src/tui/view_assembler/retained_output_view.rs"
if AEMEATH_PROJECT_DIR="$TMP_ROOT" "$TMP_ROOT/.agents/hooks/check-tui-retained-output-view.sh" >/dev/null 2>&1; then
  echo "guard should reject full root assembly" >&2
  exit 1
fi

write_valid_fixture
cat >"$TMP_ROOT/apps/cli/src/tui/render/output/document_renderer.rs" <<'EOF'
fn render_all(view_model: &OutputViewModel) {
    let semantic_root_ids = view_model.roots.iter().collect::<Vec<_>>();
}
EOF
if AEMEATH_PROJECT_DIR="$TMP_ROOT" "$TMP_ROOT/.agents/hooks/check-tui-retained-output-view.sh" >/dev/null 2>&1; then
  echo "guard should reject full semantic root scan" >&2
  exit 1
fi

write_valid_fixture
printf '%s\n' 'assemble_from_conversation();' >>"$TMP_ROOT/apps/cli/src/tui/app/update.rs"
if AEMEATH_PROJECT_DIR="$TMP_ROOT" "$TMP_ROOT/.agents/hooks/check-tui-retained-output-view.sh" >/dev/null 2>&1; then
  echo "guard should reject production full assembly" >&2
  exit 1
fi

write_valid_fixture
printf '%s\n' 'struct OutputViewCache;' >>"$TMP_ROOT/apps/cli/src/tui/app/update.rs"
if AEMEATH_PROJECT_DIR="$TMP_ROOT" "$TMP_ROOT/.agents/hooks/check-tui-retained-output-view.sh" >/dev/null 2>&1; then
  echo "guard should reject retired output cache" >&2
  exit 1
fi

write_valid_fixture
sed -i.bak '/pop_front/d' "$TMP_ROOT/apps/cli/src/tui/model/conversation/output_view_change.rs"
rm -f "$TMP_ROOT/apps/cli/src/tui/model/conversation/output_view_change.rs.bak"
if AEMEATH_PROJECT_DIR="$TMP_ROOT" "$TMP_ROOT/.agents/hooks/check-tui-retained-output-view.sh" >/dev/null 2>&1; then
  echo "guard should reject unbounded journal" >&2
  exit 1
fi

echo "TUI retained output view guard tests passed."
