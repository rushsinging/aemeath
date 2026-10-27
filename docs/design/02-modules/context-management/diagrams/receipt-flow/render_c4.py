#!/usr/bin/env python3
"""receipt-flow-c4.puml → receipt-flow-c4.svg（PlantUML 在线渲染，无需本地 Java）。

用法：python3 render_c4.py
依赖：网络（plantuml.com 公共渲染服务）。
踩坑记录（改动前必读）：
  1. 公共服务器禁远程 !include（GitHub raw 会 400）——必须用 stdlib `!include <C4/...>`；
  2. stdlib 宏分层：Person/System 在 C4_Context，Container/SystemDb 在 C4_Container，
     Component/ComponentDb 在 C4_Component——用 Component 必须引 C4_Component；
  3. GET 编码 URL（deflate + 自定义 base64）；POST 表单返回广告页不可用。
"""
import pathlib
import urllib.request
import zlib

HERE = pathlib.Path(__file__).resolve().parent
SRC = HERE / "receipt-flow-c4.puml"
OUT = HERE / "receipt-flow-c4.svg"

ALPHABET = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz-_"


def encode64(data: bytes) -> str:
    out = bytearray()
    for i in range(0, len(data), 3):
        b1 = data[i]
        b2 = data[i + 1] if i + 1 < len(data) else 0
        b3 = data[i + 2] if i + 2 < len(data) else 0
        out.append(ALPHABET[b1 >> 2])
        out.append(ALPHABET[((b1 & 0x3) << 4) | (b2 >> 4)])
        c = ALPHABET[((b2 & 0xF) << 2) | (b3 >> 6)]
        d = ALPHABET[b3 & 0x3F]
        if i + 1 >= len(data):
            c = ord("_")
            d = ord("_")
        elif i + 2 >= len(data):
            d = ord("_")
        out.append(c)
        out.append(d)
    return out.decode()


def plantuml_encode(text: str) -> str:
    compressor = zlib.compressobj(9, zlib.DEFLATED, -15)
    return encode64(compressor.compress(text.encode()) + compressor.flush())


def main() -> None:
    encoded = plantuml_encode(SRC.read_text())
    url = "https://www.plantuml.com/plantuml/svg/" + encoded
    request = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
    svg = urllib.request.urlopen(request, timeout=90).read()
    if b"<svg" not in svg[:600]:
        raise SystemExit(f"渲染失败（非 SVG 输出）：{svg[:200]!r}")
    OUT.write_bytes(svg)
    print(f"{OUT.name}: {len(svg)} bytes")


if __name__ == "__main__":
    main()
