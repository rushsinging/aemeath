#!/bin/bash
# guard-registry:policy.config.override-store.composition-ownership
set -euo pipefail

# Config owns override key/codec/error semantics, but Composition alone selects
# the filesystem-backed AtomicBlob implementation used by deployable bootstrap.
ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$ROOT"

python3 - <<'PY'
from pathlib import Path
import json, re, sys

root = Path.cwd()
config_app = root / "agent/features/config/src/application.rs"
composition_app = root / "agent/composition/src/app.rs"
violations = []

# ---------------------------------------------------------------------------
# Helpers — blank-then-scan: blank comments/strings, then simple brace counting
#
# Previous guard used a 220-line character-by-character Rust parser that walked
# the text skipping strings, char literals, lifetimes, comments, and nested
# generics while tracking brace depth.  This rewrite replaces all of that with
# a single pre-pass that blanks out every comment / string / char literal
# (preserving text length), then does trivial brace-depth tracking on the
# blanked copy.  Word-boundary \b on the fn regex replaces the hand-rolled
# `_at_fn_keyword` sentinel.
# ---------------------------------------------------------------------------

def production_text(text):
    """Drop everything after the first `#[cfg(test)]` attribute."""
    return text.split("#[cfg(test)]", 1)[0]


def _blank_comments_and_strings(text):
    """Return a copy of *text* with every Rust comment / string literal / char
    literal replaced by spaces.  Length is preserved so indices stay valid for
    slicing the original text.  Lifetimes (`'a`, `'static`) carry no brace risk
    and are left untouched."""
    out = list(text)
    i, n = 0, len(text)
    while i < n:
        c = text[i]
        # -- line comment  // ...
        if c == '/' and i + 1 < n and text[i + 1] == '/':
            while i < n and text[i] != '\n':
                out[i] = ' '; i += 1
            continue
        # -- block comment  /* ... */
        if c == '/' and i + 1 < n and text[i + 1] == '*':
            out[i] = out[i + 1] = ' '
            i += 2
            while i < n - 1 and not (text[i] == '*' and text[i + 1] == '/'):
                out[i] = ' '; i += 1
            if i < n - 1:
                out[i] = out[i + 1] = ' '
                i += 2
            continue
        # -- string literal  "..."
        if c == '"':
            out[i] = ' '
            i += 1
            while i < n:
                if text[i] == '\\' and i + 1 < n:
                    out[i] = out[i + 1] = ' '
                    i += 2
                elif text[i] == '"':
                    out[i] = ' '
                    i += 1
                    break
                else:
                    out[i] = ' '
                    i += 1
            continue
        # -- char literal  'x'  (but NOT lifetime  'a  or  'static)
        if c == "'":
            if i + 1 < n and text[i + 1].isalpha() and (i + 2 >= n or text[i + 2] not in ("'", "\\")):
                i += 1          # lifetime — skip the apostrophe
                continue
            out[i] = ' '
            i += 1
            while i < n:
                if text[i] == '\\' and i + 1 < n:
                    out[i] = out[i + 1] = ' '
                    i += 2
                elif text[i] == "'":
                    out[i] = ' '
                    i += 1
                    break
                else:
                    out[i] = ' '
                    i += 1
            continue
        i += 1
    return ''.join(out)


_FN_RE = re.compile(r'\bfn\s+([A-Za-z_]\w*)\s*\(')

def _find_function_bodies(text):
    """Yield (name, params, body_start, body_end) for each top-level `fn`.

    Uses a comment/string-blanked copy for safe brace-depth tracking;
    *body_start* / *body_end* are indices into *text* excluding the outer
    `{` `}`.  *params* is the raw parameter-list text from *text*.
    """
    blanked = _blank_comments_and_strings(text)
    for m in _FN_RE.finditer(blanked):
        name = m.group(1)
        # Walk to matching `)` — safe on blanked (no nested parens in strings)
        depth, j = 1, m.end()
        while j < len(blanked) and depth > 0:
            if blanked[j] == '(':   depth += 1
            elif blanked[j] == ')': depth -= 1
            j += 1
        if depth != 0:
            continue
        # Find `{` (body) or `;` (trait-method decl without body)
        k = j
        while k < len(blanked) and blanked[k] not in ('{', ';'):
            k += 1
        if k >= len(blanked) or blanked[k] != '{':
            continue
        # Walk to matching `}`
        depth, p = 1, k + 1
        while p < len(blanked) and depth > 0:
            if blanked[p] == '{':   depth += 1
            elif blanked[p] == '}': depth -= 1
            p += 1
        params = text[m.end() : j - 1]
        yield (name, params, k + 1, p - 1)


# ---------------------------------------------------------------------------
# Self-tests — verify the blank+scan approach against known regressions
# ---------------------------------------------------------------------------

def _self_test():
    fails = []
    def ok(cond, label):
        if not cond: fails.append(label)

    # Probe 1 — happy path: factory constructs NativeConfigStore from blob;
    # bootstrap calls factory then wiring, all within one body.
    src = (
        "fn wire_config_override_store(agents_dir: &Path) -> Result<NativeConfigStore, Error> {\n"
        '    let blob = storage::api::file_system_blob(agents_dir.join("config-overrides"))?;\n'
        "    Ok(NativeConfigStore::new(blob))\n"
        "}\n"
        "fn build(agents_dir: &Path) -> Result<Wiring, Error> {\n"
        "    let store = wire_config_override_store(agents_dir)?;\n"
        "    wire_project_config_with_cli(&cwd, store, cli)\n"
        "}\n"
    )
    fns = list(_find_function_bodies(src))
    factories = [f for f in fns if f[0] == "wire_config_override_store"]
    ok(len(factories) == 1, f"probe-good: factory count ({len(factories)})")
    if factories:
        body = src[factories[0][2]:factories[0][3]]
        ok("NativeConfigStore::new" in body, "probe-good: factory body missing NativeConfigStore::new")
        ok("file_system_blob" in body, "probe-good: factory body missing file_system_blob")
    for name, _p, bs, be in fns:
        if name != "build": continue
        body = src[bs:be]
        if re.search(r"\bwire_project_config_with_(?:cli|agents_dir)\s*\(", body):
            ok(bool(re.search(r"\bwire_config_override_store\s*\(", body)),
               "probe-good: wiring fn without same-body factory call")

    # Probe 2 — cross-function regression.  Wiring in bootstrap, factory
    # defined separately.  Structured parser MUST reject.
    src2 = (
        'fn wire_config_override_store(agents_dir: &Path) -> Result<NativeConfigStore, Error> {\n'
        '    let blob = storage::api::file_system_blob(agents_dir.join("x"))?;\n'
        "    Ok(NativeConfigStore::new(blob))\n"
        "}\n"
        "fn bootstrap() -> Result<Wiring, Error> {\n"
        "    let store = make_default_store();\n"
        "    wire_project_config_with_cli(&cwd, store, cli)\n"
        "}\n"
    )
    rejected = False
    for name, _p, bs, be in _find_function_bodies(src2):
        if name != "bootstrap": continue
        body = src2[bs:be]
        if re.search(r"\bwire_project_config_with_(?:cli|agents_dir)\s*\(", body):
            if not re.search(r"\bwire_config_override_store\s*\(", body):
                rejected = True
    ok(rejected, "probe-cross-fn: failed to flag missing same-body factory")

    # Probe 3 — factory body does NOT construct NativeConfigStore via ::new
    src3 = (
        "fn wire_config_override_store(agents_dir: &Path) -> Result<NativeConfigStore, Error> {\n"
        "    Ok(NativeConfigStore::default())\n"
        "}\n"
    )
    factories = [f for f in _find_function_bodies(src3) if f[0] == "wire_config_override_store"]
    ok(len(factories) == 1, f"probe-no-new: factory count ({len(factories)})")
    if factories:
        body = src3[factories[0][2]:factories[0][3]]
        ok("NativeConfigStore::new" not in body,
           "probe-no-new: missed NativeConfigStore::new absence")

    # Probe 4 — #[cfg(test)] stripping
    src4 = (
        "fn wire_config_override_store(agents_dir: &Path) -> Result<NativeConfigStore, Error> {\n"
        "    Ok(NativeConfigStore::new(blob))\n"
        "}\n"
        "fn build(agents_dir: &Path) -> Result<Wiring, Error> {\n"
        "    let store = wire_config_override_store(agents_dir)?;\n"
        "    wire_project_config_with_cli(&cwd, store, cli)\n"
        "}\n"
        "#[cfg(test)]\n"
        "mod tests {\n"
        "    use super::*;\n"
        "    fn helper(agents_dir: &Path) -> Result<Wiring, Error> {\n"
        "        let store = wire_config_override_store(agents_dir)?;\n"
        "        wire_project_config_with_agents_dir(&cwd, agents_dir, store, cli)\n"
        "    }\n"
        "}\n"
    )
    stripped = production_text(src4)
    ok("wire_project_config_with_agents_dir" not in stripped,
       "probe-cfg-test: test region not stripped")
    ok("wire_config_override_store" in stripped,
       "probe-cfg-test: strip over-reached into production")

    # Probe 5 — braces inside string literals / char literals / block comments
    # must not perturb depth tracking.
    src5 = (
        'fn example() {\n'
        '    let s = "hello { world }";\n'
        "    let c = '{';\n"
        "    /* outer { comment with } */\n"
        "    let real = {\n"
        "        42\n"
        "    };\n"
        "}\n"
        'fn next() {\n'
        '    let marker = "} {";\n'
        "    1\n"
        "}\n"
    )
    fns5 = list(_find_function_bodies(src5))
    ok(len(fns5) == 2, f"probe-tricky: expected 2 fns, got {len(fns5)}")
    if len(fns5) >= 1:
        ok(fns5[0][0] == "example", "probe-tricky: first fn name mismatch")
        body = src5[fns5[0][2]:fns5[0][3]]
        ok("42" in body and '"hello { world }"' in body,
           "probe-tricky: brace tracking mis-scoped body")
    if len(fns5) >= 2:
        ok(fns5[1][0] == "next", "probe-tricky: second fn name mismatch")

    # Probe 6 — `fn` substring inside an identifier must not yield a truncated name
    src6 = (
        "pub fn some_fn_ptr() -> usize { 0 }\n"
        "pub fn real() -> usize { 1 }\n"
    )
    names = [f[0] for f in _find_function_bodies(src6)]
    ok("fn_ptr" not in names, "probe-no-substr: captured truncated fn_ptr")
    ok("real" in names, "probe-no-substr: real fn not parsed")
    ok("some_fn_ptr" in names,
       f"probe-no-substr: some_fn_ptr missing; got {','.join(names)}")
    return fails


self_failures = _self_test()
if self_failures:
    print(json.dumps({
        "decision": "block",
        "reason": "Config override store ownership guard self-test FAILED:\n"
                  + "\n".join(self_failures)
    }, ensure_ascii=False))
    sys.exit(2)


# ---------------------------------------------------------------------------
# Real checks
# ---------------------------------------------------------------------------

NATIVE_STORE_PARAM = re.compile(r"\bnative_store\s*:\s*NativeConfigStore\b")

# -- Config application -------------------------------------------------
if not config_app.is_file():
    violations.append("agent/features/config/src/application.rs: Config application source is missing")
else:
    prod = production_text(config_app.read_text())
    if re.search(r"\b(?:storage::api::)?file_system_blob\s*\(", prod):
        violations.append(
            "agent/features/config/src/application.rs: Config application must consume injected NativeConfigStore, not construct file_system_blob"
        )
    if re.search(r"\bFileSystemBlobAdapter::new\s*\(", prod):
        violations.append(
            "agent/features/config/src/application.rs: Config application must not construct FileSystemBlobAdapter"
        )

    wiring_fns = {"wire_project_config_with_cli", "wire_project_config", "for_project"}
    found = {n: False for n in wiring_fns}
    for name, params, bs, be in _find_function_bodies(prod):
        if name not in wiring_fns:
            continue
        found[name] = True
        if not NATIVE_STORE_PARAM.search(params):
            violations.append(
                f"agent/features/config/src/application.rs: {name}() must declare a `native_store: NativeConfigStore` parameter"
            )
        body = prod[bs:be]
        if re.search(r"\bfile_system_blob\s*\(", body):
            violations.append(
                f"agent/features/config/src/application.rs: {name}() must not construct file_system_blob; consume the injected native_store"
            )
    for name in wiring_fns:
        if not found[name]:
            violations.append(
                f"agent/features/config/src/application.rs: missing required wiring function {name}()"
            )

# -- Composition app ----------------------------------------------------------
if not composition_app.is_file():
    violations.append("agent/composition/src/app.rs: Composition app source is missing")
else:
    text = composition_app.read_text()
    prod = production_text(text)
    fns = list(_find_function_bodies(prod))

    factories = [f for f in fns if f[0] == "wire_config_override_store"]
    if len(factories) != 1:
        violations.append(
            f"agent/composition/src/app.rs: Composition must define exactly one `wire_config_override_store` factory in production (found {len(factories)})"
        )
    elif factories:
        _name, _params, bs, be = factories[0]
        body = prod[bs:be]
        if "NativeConfigStore::new" not in body:
            violations.append(
                "agent/composition/src/app.rs: wire_config_override_store() must construct NativeConfigStore inside its function body"
            )
        if "file_system_blob" not in body:
            violations.append(
                "agent/composition/src/app.rs: wire_config_override_store() must select the filesystem-backed blob inside its function body"
            )

    # Same-body wiring invariant — scoped per function so a
    # wire_project_config_with_* call cannot reach across function boundaries
    # for a wire_config_override_store defined elsewhere.
    wiring_re = re.compile(r"\bwire_project_config_with_(?:cli|agents_dir)\s*\(")
    factory_re = re.compile(r"\bwire_config_override_store\s*\(")
    for name, _params, bs, be in fns:
        body = prod[bs:be]
        if not wiring_re.search(body):
            continue
        if not factory_re.search(body):
            violations.append(
                f"agent/composition/src/app.rs: production function `{name}` calls wire_project_config_with_* without calling wire_config_override_store in the same body; the factory result must be forwarded within the same function body"
            )

    if re.search(r"\bwire_project_config_with_agents_dir\s*\(", prod):
        violations.append(
            "agent/composition/src/app.rs: production source must not call `wire_project_config_with_agents_dir`; it is only wired from #[cfg(test)] paths in #1385"
        )

# ---------------------------------------------------------------------------
if violations:
    print(json.dumps({
        "decision": "block",
        "reason": "Config override store ownership guard FAILED:\n" + "\n".join(violations)
    }, ensure_ascii=False))
    sys.exit(2)

print("Config override store ownership guard OK.")
PY
