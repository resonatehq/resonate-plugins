#!/usr/bin/env python3
"""Mechanical checks on a plugin's crate against its specification. Usage:

    python3 skills/plugin-implement/check.py plugins/<name>

Exit 0 = clean. Errors are rule violations; warnings need a human eye.

skills/plugin-specify/lint.py checks the specification against itself.
This checks the crate against the specification: that every operation was
translated, that no provider fact reached the code without passing through
the document, and that the crate is the shape the skill prescribes. It
does not compile or run anything — `cargo test` is the other half.
"""
import ast, json, os, re, subprocess, sys

def fences(text, lang):
    return re.findall(rf"```{lang}\n(.*?)\n```", text, re.S)

def main(root):
    name = os.path.basename(root.rstrip("/"))
    spec_p = f"{root}/spec/specification.md"
    rs_p   = f"{root}/src/src/plugin.rs"
    errs, warns, notes = [], [], []

    if not os.path.exists(spec_p): return print(f"{name}: no specification"), 1
    spec = open(spec_p).read()
    py   = "\n".join(fences(spec, "python"))
    ops  = re.findall(r"^### 4\.\d+ ([a-z0-9]+\.[a-z]+)$", spec, re.M)

    # --- crate shape --------------------------------------------------
    for f in ["Cargo.toml", "src/lib.rs", "src/worker.rs", "src/plugin.rs",
              ".cargo/config.toml", "tests/process.rs", "tests/e2e.rs"]:
        if not os.path.exists(f"{root}/src/{f}"):
            errs.append(f"crate: missing {f}")
    if not os.path.exists(rs_p): return _report(name, errs, warns, notes)
    rs = open(rs_p).read()

    # C-1 template placeholders replaced
    for f in ["Cargo.toml", "src/lib.rs", "src/plugin.rs"]:
        p = f"{root}/src/{f}"
        if os.path.exists(p) and "{name}" in open(p).read():
            errs.append(f"C-1: {f} still contains the '{{name}}' placeholder")
    lib = open(f"{root}/src/src/lib.rs").read() if os.path.exists(f"{root}/src/src/lib.rs") else ""
    m = re.search(r'SCHEME: &str = "([a-z0-9]+)"', lib)
    if m and m.group(1) != name:
        errs.append(f"C-1: SCHEME is '{m.group(1)}', expected '{name}'")

    # C-2 worker.rs is the untouched frame
    ref = "skills/plugin-implement/reference/src/worker.rs"
    if os.path.exists(ref):
        a = open(ref).read().replace("{name}", name)
        if a.strip() != open(f"{root}/src/src/worker.rs").read().strip():
            warns.append("C-2: src/worker.rs differs from the reference frame")

    # C-3 one match arm per specification operation
    arms = set(re.findall(r'"([a-z0-9]+\.[a-z]+)"\s*=>', rs))
    for op in ops:
        if op not in arms:
            errs.append(f"C-3: operation {op} has no match arm in process()")
    for a in arms - set(ops):
        errs.append(f"C-3: match arm '{a}' is not a specification operation")

    # C-4 rejection codes: specification python <-> rust
    spec_codes = set(re.findall(r'"code":\s*"([a-z_]+)"', py))
    rust_codes = (set(re.findall(r'"code":\s*"([a-z_]+)"', rs))
                  | set(re.findall(r'reject\(\s*"([a-z_]+)"', rs))
                  | set(re.findall(r'"([a-z_]+)"\s*\}?\s*else', rs))
                  | set(re.findall(r'=>\s*"([a-z_]+)"', rs)))
    frame = {"unknown_func", "invalid_request"}
    for c in sorted(spec_codes - rust_codes - frame):
        errs.append(f"C-4: code '{c}' is constructed in the specification but not in the crate")
    for c in sorted(rust_codes - spec_codes - frame):
        if c in ("halt", "release", "resolved", "rejected"): continue
        warns.append(f"C-4: code '{c}' is constructed in the crate but not in the specification")

    # C-5 no provider fact in the crate that is not in the specification
    paths = set(re.findall(r'"(/[A-Za-z0-9_./{}-]{3,})"', rs)) | set(re.findall(r'\{\}(/[A-Za-z0-9_./-]{3,})', rs))
    for p in sorted(paths):
        stem = re.split(r"\{|\}", p)[0].rstrip("/")
        if len(stem) > 3 and stem not in spec:
            errs.append(f"C-5: crate uses path '{p}' which the specification never mentions")

    # C-6 forbidden shapes in the crate
    if ".unwrap()" in rs:
        warns.append(f"C-6: {rs.count('.unwrap()')} .unwrap() call(s) in plugin.rs")
    if "todo!" in rs or "unimplemented!" in rs:
        errs.append("C-6: plugin.rs still contains todo!/unimplemented!")

    # C-7 every poll loop is bounded by timeout_at
    if "request_poll" in spec and "timeout_at" not in rs:
        errs.append("C-7: specification has a request_poll operation but the crate never reads timeout_at")

    # C-8 tests: one per (operation x documented condition), plus e2e
    tp = open(f"{root}/src/tests/process.rs").read() if os.path.exists(f"{root}/src/tests/process.rs") else ""
    te = open(f"{root}/src/tests/e2e.rs").read() if os.path.exists(f"{root}/src/tests/e2e.rs") else ""
    n_tp = len(re.findall(r"#\[tokio::test", tp)) + len(re.findall(r"#\[test\]", tp))
    n_te = len(re.findall(r"#\[tokio::test", te)) + len(re.findall(r"#\[test\]", te))
    notes.append(f"tests: {n_tp} in process.rs, {n_te} in e2e.rs, for {len(ops)} operations "
                 f"and {len(spec_codes)} rejection codes")
    if n_te < 2:
        errs.append(f"C-8: e2e.rs has {n_te} test(s); the skill prescribes a resolved and a rejected path")
    if n_tp < len(ops) + len(spec_codes):
        warns.append(f"C-8: {n_tp} process tests for {len(ops)} operations + {len(spec_codes)} codes")
    if "wiremock" in tp and "Self-hosted** | yes" in spec:
        errs.append("C-8: specification is self-hosted (§5) but process.rs uses wiremock")

    # C-9 nothing gitignored got committed
    tracked = subprocess.run(["git", "ls-files", root], capture_output=True, text=True).stdout.split()
    for f in tracked:
        chk = subprocess.run(["git", "check-ignore", "--no-index", "-q", f])
        if chk.returncode == 0:
            errs.append(f"C-9: {f} is tracked but matches .gitignore")

    # C-10 Reviewed by
    rb = re.search(r"\| \*\*Reviewed by\*\* \|(.*?)\|", spec)
    if not rb or not rb.group(1).strip():
        errs.append("C-10: Reviewed by is empty — the specification was never reviewed")
    elif not re.fullmatch(r"[A-Za-z0-9 .]+, \d{4}-\d{2}-\d{2}", rb.group(1).strip()):
        warns.append(f"C-10: Reviewed by '{rb.group(1).strip()}' is not '<Model Name>, YYYY-MM-DD'")

    return _report(name, errs, warns, notes)

def _report(name, errs, warns, notes):
    for n in notes: print(f"        {n}")
    for e in errs:  print(f"ERROR   {e}")
    for w in warns: print(f"warning {w}")
    print(f"{name}: {len(errs)} errors, {len(warns)} warnings")
    return 1 if errs else 0

if __name__ == "__main__":
    sys.exit(main(sys.argv[1]))
