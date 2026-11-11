#!/usr/bin/env python3
"""Attribute every byte of a macOS executable from its ld64 link map (`-C link-arg=-Wl,-map,<file>` on an unstripped
build): sections, per-crate totals, the largest symbols, embedded data, and generic instantiation counts.

  python3 bench/size/attribute.py <map> [--top 40] [--json out.json]

Crates: with fat LTO every Rust symbol comes from one object, so a symbol's crate is the first path of its demangled
name (`<bough_text::pool::GlyphPool>::mark_window::<..>` -> bough_text; `<T as Trait>::f` -> T's crate; demangled by
the Command Line Tools' `llvm-cxxfilt`). Inlined code counts where it landed. Without LTO a symbol's crate is its
object file's. Anonymous constants (`l_anon.<cgu hash>.<n>`) carry no name: they are grouped by section and by the
hash of the codegen unit that emitted them, and the largest are reported by content (`--blobs` prints a prefix).
Generic instantiations: a demangled name with generic arguments (`::<..>` or `<T<..>>::`) is one instantiation of
the generic item named by the name with every `<..>` argument list removed."""
import argparse, collections, json, os, re, subprocess, sys

CXXFILT = "/Library/Developer/CommandLineTools/usr/bin/llvm-cxxfilt"
DATA_SECTIONS = {"__const", "__cstring", "__data", "__thread_data", "__gcc_except_tab", "__eh_frame", "__unwind_info"}


def parse(path):
    objs, sects, syms = {}, [], []
    mode = None
    with open(path, "rb") as f:
        for raw in f:
            line = raw.decode("utf-8", "replace").rstrip("\n")
            if line.startswith("# Object files:"):
                mode = "o"; continue
            if line.startswith("# Sections:"):
                mode = "s"; continue
            if line.startswith("# Symbols:"):
                mode = "y"; continue
            if line.startswith("# Dead Stripped Symbols:"):
                mode = None; continue
            if line.startswith("#") or not line:
                continue
            if mode == "o":
                m = re.match(r"\[\s*(\d+)\]\s+(.*)", line)
                if m:
                    objs[int(m.group(1))] = m.group(2)
            elif mode == "s":
                p = line.split("\t")
                sects.append((int(p[0], 16), int(p[1], 16), p[2], p[3]))
            elif mode == "y":
                p = line.split("\t", 3)
                if len(p) < 3:
                    continue
                m = re.match(r"\[\s*(\d+)\]\s?(.*)", p[2] if len(p) == 3 else p[2] + "\t" + p[3])
                syms.append((int(p[0], 16), int(p[1], 16), int(m.group(1)), m.group(2)))
    return objs, sects, syms


def section_of(sects, addr):
    for a, s, seg, sec in sects:
        if a <= addr < a + max(s, 1):
            return seg + "," + sec
    return "?"


def demangle(names):
    todo = [n for n in names if n.startswith("__R") or n.startswith("__ZN")]
    out = {}
    if todo:
        p = subprocess.run([CXXFILT, "-_"], input="\n".join(todo), capture_output=True, text=True)
        for a, b in zip(todo, p.stdout.split("\n")):
            out[a] = b
    return out


def strip_hash(s):
    return re.sub(r"\[[0-9a-f]{16}\]", "", s)


def crate_of_name(d):
    s = d
    for _ in range(8):
        s = s.lstrip("<&*([ ")
        for pre in ("mut ", "const ", "dyn ", "unsafe ", "extern \"C\" ", "fn"):
            if s.startswith(pre):
                s = s[len(pre):]
    m = re.match(r"([A-Za-z_][A-Za-z0-9_]*)(\[[0-9a-f]+\])?::", s)
    if m:
        return m.group(1)
    return None


def crate_of_obj(path):
    b = os.path.basename(path)
    m = re.match(r"lib(.+?)-[0-9a-f]+\.rlib\(", b)
    if m:
        return m.group(1)
    m = re.match(r"([A-Za-z0-9_]+?)-[0-9a-f]+\.", b)
    if m:
        return m.group(1)
    if b.endswith(".tbd"):
        return "(dylib stub) " + b[:-4]
    m = re.match(r"([A-Za-z0-9_]+)\.", b)
    return m.group(1) if m else b


def generic_key(d):
    """The generic item a demangled name instantiates (argument lists removed), or None if it has none."""
    if "<" not in d:
        return None
    out, depth = [], 0
    for ch in d:
        if ch == "<":
            depth += 1
            out.append("<") if depth == 1 else None
            continue
        if ch == ">":
            depth -= 1
            out.append(">") if depth == 0 else None
            continue
        if depth == 0:
            out.append(ch)
    k = "".join(out)
    # `<X>::f` with X a plain path is an inherent impl, not a generic argument: keep only argument-bearing names
    has_args = "::<" in d or re.search(r"[A-Za-z0-9_]<", d) is not None
    return k if has_args else None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("map")
    ap.add_argument("--top", type=int, default=40)
    ap.add_argument("--json")
    ap.add_argument("--blobs", type=int, default=12, help="largest anonymous constants to print with a content prefix")
    ap.add_argument("--binary", help="the unstripped executable, to read anonymous constants' content")
    a = ap.parse_args()
    objs, sects, syms = parse(a.map)
    lto = sum(1 for o in objs.values() if ".rcgu.o" in o and "rlib(" not in o) == 1 and len([o for o in objs.values() if "rlib(" in o and "compiler_builtins" not in o]) == 0
    names = demangle([s[3] for s in syms])
    rows = []
    for addr, size, f, name in syms:
        sec = section_of(sects, addr)
        d = strip_hash(names.get(name, name))
        if name.startswith("literal string:"):
            kind, crate = "cstring", None
        elif name.startswith("l_anon.") or name == "anon" or name.startswith("ltmp") or name.startswith("_anon"):
            kind, crate = "anon", None
        elif name.startswith("GCC_except_table"):
            kind, crate = "lsda", None
        elif name in ("CIE", "FDE") or name.startswith("CIE") or name.startswith("FDE"):
            kind, crate = "unwind", None
        else:
            kind = "sym"
            crate = crate_of_name(d) if (lto and f == 1) or name in names else None
        if crate is None and kind == "sym":
            o = objs.get(f, "?")
            crate = crate_of_obj(o) if f != 0 else "(linker)"
            if name.startswith("_objc_msgSend$"):
                crate = "(objc selector stubs)"
        if crate is None and not lto:
            crate = crate_of_obj(objs.get(f, "?")) if f != 0 else "(linker)"
        rows.append(dict(addr=addr, size=size, file=f, name=name, dem=d, sec=sec, kind=kind, crate=crate))

    total = sum(s for _, s, _, _ in sects if not ("__bss" in _ or "zerofill" in _))
    print(f"## {os.path.basename(a.map)}  (LTO single object: {lto})\n")
    print("| section | bytes |\n|---|---:|")
    for addr, size, seg, sec in sects:
        print(f"| {seg},{sec} | {size:,} |")
    covered = collections.Counter()
    for r in rows:
        covered[r["sec"]] += r["size"]
    print("\nmap symbols cover per section: " + ", ".join(f"{k} {v:,}" for k, v in covered.most_common()))

    # per crate: code and data
    code, data = collections.Counter(), collections.Counter()
    for r in rows:
        key = r["crate"] or f"({r['kind']})"
        if r["sec"].endswith(",__text"):
            code[key] += r["size"]
        else:
            data[key + "  @" + r["sec"]] += r["size"]
    print("\n### __text by crate (code; inlined code counts where it landed)\n\n| crate | bytes | share |\n|---|---:|---:|")
    tt = sum(code.values()) or 1
    for k, v in code.most_common():
        if v >= 512:
            print(f"| {k} | {v:,} | {100 * v / tt:.1f} % |")
    small = sum(v for v in code.values() if v < 512)
    print(f"| (crates under 512 B) | {small:,} | |")
    print("\n### data by crate and section (named symbols; anonymous constants grouped by kind)\n\n| crate @ section | bytes |\n|---|---:|")
    for k, v in data.most_common(40):
        print(f"| {k} | {v:,} |")

    print(f"\n### the {a.top} largest symbols\n\n| # | bytes | section | crate | symbol |\n|---:|---:|---|---|---|")
    for i, r in enumerate(sorted(rows, key=lambda r: -r["size"])[: a.top]):
        n = r["dem"] if r["kind"] == "sym" else r["name"]
        n = n.replace("|", "\\|")
        if len(n) > 160:
            n = n[:157] + "..."
        print(f"| {i + 1} | {r['size']:,} | {r['sec']} | {r['crate'] or r['kind']} | `{n}` |")

    # anonymous constants by CGU hash
    anon = collections.Counter()
    for r in rows:
        if r["kind"] == "anon":
            m = re.match(r"l_anon\.([0-9a-f]+)\.", r["name"])
            anon[(r["sec"], m.group(1)[:8] if m else r["name"])] += r["size"]
    print("\n### anonymous constants by section and emitting codegen unit (hash prefix)\n\n| section | cgu | bytes |\n|---|---|---:|")
    for (sec, h), v in anon.most_common(15):
        print(f"| {sec} | {h} | {v:,} |")
    if a.binary and a.blobs:
        print("\n### the largest anonymous constants, by content\n")
        big = sorted((r for r in rows if r["kind"] == "anon"), key=lambda r: -r["size"])[: a.blobs]
        seg_off = macho_segments(a.binary)
        for r in big:
            pre = read_at(a.binary, seg_off, r["addr"], 48)
            print(f"* {r['size']:,} B {r['sec']} {r['name']}: `{printable(pre)}`")

    # generic instantiations
    gen = collections.defaultdict(lambda: [0, 0])
    text_syms = [r for r in rows if r["sec"].endswith(",__text") and r["kind"] == "sym"]
    for r in text_syms:
        k = generic_key(r["dem"])
        if k:
            gen[k][0] += 1
            gen[k][1] += r["size"]
    ninst = sum(v[0] for v in gen.values())
    nbytes = sum(v[1] for v in gen.values())
    print(f"\n### generic instantiations in __text\n\n{len(text_syms):,} functions; {ninst:,} are instantiations of "
          f"{len(gen):,} generic items ({nbytes:,} B, {100 * nbytes / max(tt, 1):.1f} % of __text).\n")
    print("| generic item (arguments removed) | instances | bytes |\n|---|---:|---:|")
    for k, (n, b) in sorted(gen.items(), key=lambda kv: -kv[1][0])[:25]:
        k = k.replace("|", "\\|")
        print(f"| `{k[:150]}` | {n} | {b:,} |")
    bycrate = collections.defaultdict(lambda: [0, 0])
    for r in text_syms:
        if generic_key(r["dem"]):
            bycrate[r["crate"]][0] += 1
            bycrate[r["crate"]][1] += r["size"]
    print("\n| crate of the generic item | instances | bytes |\n|---|---:|---:|")
    for k, (n, b) in sorted(bycrate.items(), key=lambda kv: -kv[1][1])[:15]:
        print(f"| {k} | {n} | {b:,} |")
    if a.json:
        json.dump(dict(sections=[(seg + "," + sec, size) for _, size, seg, sec in sects], code=dict(code), data=dict(data),
                       generics=dict(instances=ninst, items=len(gen), bytes=nbytes)), open(a.json, "w"), indent=1)


def macho_segments(binary):
    """(vmaddr, vmsize, fileoff) of each segment, from `otool -l`."""
    out = subprocess.run(["otool", "-l", binary], capture_output=True, text=True).stdout
    segs, cur = [], {}
    for line in out.splitlines():
        line = line.strip()
        if line.startswith("cmd LC_SEGMENT_64"):
            cur = {}
        for k in ("vmaddr", "vmsize", "fileoff"):
            if line.startswith(k + " "):
                cur[k] = int(line.split()[1], 16 if "0x" in line else 10)
                if k == "fileoff":
                    segs.append((cur.get("vmaddr", 0), cur.get("vmsize", 0), cur["fileoff"]))
    return segs


def read_at(binary, segs, addr, n):
    for va, vs, fo in segs:
        if va <= addr < va + vs:
            with open(binary, "rb") as f:
                f.seek(fo + addr - va)
                return f.read(n)
    return b""


def printable(b):
    s = "".join(chr(c) if 32 <= c < 127 else "." for c in b)
    return s.replace("`", "'").replace("|", "!")


if __name__ == "__main__":
    main()
