#!/usr/bin/env python3
import json
import subprocess
import sys
from collections import defaultdict
from pathlib import Path


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: coverage_summary.py <report.json> <workspace-root>", file=sys.stderr)
        return 2

    report_path = Path(sys.argv[1])
    root = Path(sys.argv[2]).resolve()
    report = json.loads(report_path.read_text())
    workspace = json.loads(
        subprocess.check_output(
            ["cargo", "metadata", "--no-deps", "--format-version", "1"],
            cwd=root,
            text=True,
        )
    )
    packages = sorted(
        (
            package["name"],
            Path(package["manifest_path"]).resolve().parent,
        )
        for package in workspace["packages"]
    )

    metrics = ("regions", "functions", "lines")
    per_package = defaultdict(lambda: {metric: [0, 0] for metric in metrics})

    for datum in report["data"]:
        for source in datum.get("files", []):
            filename = Path(source["filename"]).resolve()
            owner = None
            owner_depth = -1
            for name, package_dir in packages:
                try:
                    filename.relative_to(package_dir)
                except ValueError:
                    continue
                depth = len(package_dir.parts)
                if depth > owner_depth:
                    owner = name
                    owner_depth = depth
            if owner is None:
                continue
            summary = source["summary"]
            for metric in metrics:
                per_package[owner][metric][0] += summary[metric]["covered"]
                per_package[owner][metric][1] += summary[metric]["count"]

    workspace_totals = {metric: [0, 0] for metric in metrics}
    for values in per_package.values():
        for metric in metrics:
            workspace_totals[metric][0] += values[metric][0]
            workspace_totals[metric][1] += values[metric][1]

    print("\nAemeath coverage summary")
    print(f"{'package':<16} {'regions':>24}  {'functions':>24}  {'lines':>24}")
    print("-" * 94)
    print(format_row("workspace", workspace_totals, metrics))
    print("-" * 94)
    for name, _ in packages:
        values = per_package[name]
        if all(values[metric][1] == 0 for metric in metrics):
            print(f"{name:<16} {'n/a':>24}  {'n/a':>24}  {'n/a':>24}")
        else:
            print(format_row(name, values, metrics))
    return 0


def format_row(label, values, metrics):
    columns = []
    for metric in metrics:
        covered, count = values[metric]
        percentage = "100.00%" if count == 0 else f"{covered / count * 100:.2f}%"
        columns.append(f"{covered}/{count} ({percentage})")
    return f"{label:<16} " + "  ".join(f"{column:>24}" for column in columns)


if __name__ == "__main__":
    raise SystemExit(main())
