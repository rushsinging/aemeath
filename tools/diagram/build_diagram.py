#!/usr/bin/env python3
"""设计图统一构建脚本（archify v3 渲染 + 详情面板注入 + JS 校验）。

用法：
  python3 tools/diagram/build_diagram.py <图目录>
    <图目录> 需含：spec.<type>.json、*-data.json、inject-*.js
    产物：index.html（archify 底图 + 注入面板，自包含可直接打开）

图目录安置规范见 tools/diagram/README.md。
"""
import json
import pathlib
import subprocess
import sys

ARCHIFY = pathlib.Path.home() / ".agents/skills/archify/bin/archify.mjs"


def build(diagram_dir: pathlib.Path) -> None:
    specs = sorted(diagram_dir.glob("spec.*.json"))
    assert len(specs) == 1, f"需要恰好一个 spec.*.json，找到 {[s.name for s in specs]}"
    spec_path = specs[0]
    diagram_type = spec_path.name.split(".")[1]
    spec = json.loads(spec_path.read_text())

    data_files = sorted(diagram_dir.glob("*-data.json"))
    assert len(data_files) == 1, f"需要恰好一个 *-data.json，找到 {[d.name for d in data_files]}"
    data_path = data_files[0]
    payload = json.loads(data_path.read_text())
    # spec label 合入 payload（节点匹配键），label 以 spec 为真相源。
    labels = {c["id"]: c.get("label") for c in spec.get("components", [])}
    labels.update({c["id"]: c.get("label") for c in spec.get("nodes", [])})
    for key, detail in payload.items():
        if key in labels and labels[key]:
            detail["label"] = labels[key]

    inject_files = sorted(diagram_dir.glob("inject-*.js"))
    assert len(inject_files) == 1, f"需要恰好一个 inject-*.js，找到 {[i.name for i in inject_files]}"

    spec["meta"].setdefault("output", "index.html")
    spec_path.write_text(json.dumps(spec, ensure_ascii=False, indent=1) + "\n")

    base_html = diagram_dir.parent.parent.parent / ".tmp" / f"{diagram_dir.name}-base.html"
    base_html.parent.mkdir(exist_ok=True)
    render = subprocess.run(
        ["node", str(ARCHIFY), "render", diagram_type, str(spec_path), str(base_html)],
        capture_output=True, text=True,
    )
    if render.returncode != 0:
        raise SystemExit(f"archify 渲染失败:\n{render.stdout}\n{render.stderr}")

    inject = inject_files[0].read_text()
    for placeholder in ("__FLOW_DATA__", "__PL_DATA__"):
        inject = inject.replace(placeholder, json.dumps(payload, ensure_ascii=False))
    html = base_html.read_text().replace(
        "</body>", "<script>\n" + inject + "\n</script>\n</body>")
    out = diagram_dir / "index.html"
    out.write_text(html)
    print(f"{out}: {len(html)} bytes（const DATA×{html.count('var DATA')}）")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit(__doc__)
    build(pathlib.Path(sys.argv[1]).resolve())
