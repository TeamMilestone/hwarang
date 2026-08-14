#!/usr/bin/env python3
"""LibreOffice hwpfilter hcode.cxx에서 HWP 3.0 자모 변환 테이블을 추출해
hwarang의 src/hwp30/tables.rs를 생성한다."""
import re
import sys

SRC = sys.argv[1] if len(sys.argv) > 1 else \
    "/Users/wonsup-mini/projects/core/hwpfilter/source/hcode.cxx"
OUT = sys.argv[2] if len(sys.argv) > 2 else \
    "src/hwp30/tables.rs"

text = open(SRC, encoding="utf-8").read()


def extract_array(name):
    """`const hchar NAME[] = { ... };` 본문에서 정수 리스트 추출"""
    m = re.search(
        r"const (?:hchar|unsigned) " + re.escape(name) + r"\[[^\]]*\]\s*=\s*\{(.*?)\};",
        text,
        re.S,
    )
    if not m:
        sys.exit(f"array {name} not found")
    body = re.sub(r"/\*.*?\*/", "", m.group(1), flags=re.S)
    body = re.sub(r"//[^\n]*", "", body)
    toks = re.findall(r"0[xX][0-9a-fA-F]+|\bnoneks\b|\d+", body)
    return [0xA1A1 if t == "noneks" else int(t, 0) for t in toks]


def extract_jamocomp(name):
    """JamoComp 구조체 배열 {size, v1, v2, v3} 추출"""
    m = re.search(
        r"const JamoComp " + re.escape(name) + r"\[\]\s*=\s*\{(.*?)\};", text, re.S
    )
    if not m:
        sys.exit(f"struct array {name} not found")
    entries = re.findall(
        r"\{\s*(\d+)\s*,\s*(0[xX][0-9a-fA-F]+)\s*,\s*(0[xX][0-9a-fA-F]+)\s*,\s*(0[xX][0-9a-fA-F]+)\s*\}",
        m.group(1),
    )
    return [(int(a), int(b, 0), int(c, 0), int(d, 0)) for a, b, c, d in entries]


choseong = extract_array("choseong_to_unicode")
joongseong = extract_array("joongseong_to_unicode")
jongseong = extract_array("jongseong_to_unicode")
jamo = extract_array("jamo_to_unicode")
comp = extract_jamocomp("jamocomp1_to_unicode")

assert len(choseong) == 32, len(choseong)
assert len(joongseong) == 32, len(joongseong)
assert len(jongseong) == 32, len(jongseong)


def fmt_u16(vals, per_line=8):
    lines = []
    for i in range(0, len(vals), per_line):
        lines.append("    " + ", ".join(f"0x{v:04x}" for v in vals[i : i + per_line]) + ",")
    return "\n".join(lines)


with open(OUT, "w", encoding="utf-8") as f:
    f.write(
        "//! HWP 3.0 조합형(KSSM) → 유니코드 변환 테이블.\n"
        "//!\n"
        "//! LibreOffice hwpfilter `hcode.cxx`에서 추출·생성됨 (MPL-2.0 / Apache-2.0).\n"
        "//! 생성 스크립트: LibreOffice core 소스 트리 필요.\n\n"
    )
    f.write(f"pub const CHOSEONG_TO_UNICODE: [u16; 32] = [\n{fmt_u16(choseong)}\n];\n\n")
    f.write(f"pub const JOONGSEONG_TO_UNICODE: [u16; 32] = [\n{fmt_u16(joongseong)}\n];\n\n")
    f.write(f"pub const JONGSEONG_TO_UNICODE: [u16; 32] = [\n{fmt_u16(jongseong)}\n];\n\n")
    f.write(
        "/// 중성이 0인 자모 영역(초성*32+종성 인덱스)\n"
        f"pub const JAMO_TO_UNICODE: [u16; {len(jamo)}] = [\n{fmt_u16(jamo)}\n];\n\n"
    )
    f.write(
        "/// 옛한글 조합 영역: (길이, 자모1, 자모2, 자모3)\n"
        f"pub const JAMOCOMP1_TO_UNICODE: [(u8, u16, u16, u16); {len(comp)}] = [\n"
    )
    for i in range(0, len(comp), 4):
        row = ", ".join(
            f"({a}, 0x{b:04x}, 0x{c:04x}, 0x{d:04x})" for a, b, c, d in comp[i : i + 4]
        )
        f.write(f"    {row},\n")
    f.write("];\n")

print(f"jamo={len(jamo)} comp={len(comp)}")


# --- HNC(한컴 2바이트 코드) → 유니코드 테이블 생성 ---
# 소스: hnc2unicode.rb (퍼블릭 도메인, Hodong Kim 작성 — Hancom Office로 생성)
# https://github.com/ddoleye/java-hwp/blob/master/src/main/resources/hnc2unicode.rb
HNC_SRC = sys.argv[3] if len(sys.argv) > 3 else None
HNC_OUT = "src/hwp30/hnc_table.rs"

if HNC_SRC:
    hnc = {}
    for line in open(HNC_SRC, encoding="utf-8"):
        m = re.match(r"\s*0x([0-9a-fA-F]{4})\s*=>\s*\[([^\]]*)\]", line)
        if m:
            code = int(m.group(1), 16)
            vals = [int(v, 16) for v in re.findall(r"0x([0-9a-fA-F]+)", m.group(2))]
            hnc[code] = vals

    # 0x0080..0x7FFF 조밀 배열. 다중 코드포인트 없음(검증됨).
    # PUA(사용자 정의 영역) 매핑은 추출 텍스트에 무의미하므로 0(미매핑) 처리.
    dense = []
    for code in range(0x80, 0x8000):
        vals = hnc.get(code, [])
        v = vals[0] if len(vals) == 1 else 0
        if v > 0xFFFF or 0xE000 <= v <= 0xF8FF:
            v = 0
        dense.append(v)

    with open(HNC_OUT, "w", encoding="utf-8") as f:
        f.write(
            "//! HNC(한컴 2바이트 코드) 0x0080~0x7FFF → 유니코드 조밀 테이블.\n"
            "//!\n"
            "//! `hnc2unicode.rb`(퍼블릭 도메인, Hodong Kim — Hancom Office 2014로 생성)에서\n"
            "//! 추출. 0 = 매핑 없음(PUA 포함). 인덱스 = HNC 코드 - 0x80.\n"
            "//! 생성 스크립트: `scripts/gen_hwp30_tables.py`\n\n"
            "pub static HNC_TO_UNICODE_LO: [u16; 0x7F80] = [\n"
        )
        for i in range(0, len(dense), 8):
            f.write("    " + ", ".join(f"0x{v:04x}" for v in dense[i : i + 8]) + ",\n")
        f.write("];\n")
    print(f"hnc dense entries={len(dense)} nonzero={sum(1 for v in dense if v)}")
