#!/usr/bin/env python3
"""Mechanical checks for a plugin specification. Usage:

    python3 skills/plugin-specify/lint.py [--pre-review] plugins/<name>/spec/specification.md

Exit 0 = clean. Errors are rule violations; warnings need a human eye.

--pre-review is for the specification as its author leaves it, before
`plugin-review` has run: it promotes L-14 (Reviewed by must be empty) from
a warning to an error, so an author who attests to their own review fails
the check rather than passing it with a warning nobody reads.
"""

import ast
import json
import re
import sys

errors = []
warnings = []


def err(msg):
    errors.append(msg)


def warn(msg):
    warnings.append(msg)


def fences(text, lang):
    return [m.group(1) for m in re.finditer(rf"```{lang}\n(.*?)\n```", text, re.S)]


def main(path, pre_review=False):
    text = open(path).read()

    # L-1: heading skeleton
    heads = re.findall(r"^(#{2,4}) (.+)$", text, re.M)
    h2 = [t for lvl, t in heads if lvl == "##"]
    expected = ["1. Address", "2. Configuration", "3. Authentication & Authorization", "4. Operations"]
    if [h for h in h2 if h[0].isdigit()][: len(expected)] != expected:
        err(f"L-1: top-level sections must start {expected}, got {h2}")
    has_s5 = any(h.startswith("5. Test") for h in h2)
    selfhosted = re.search(r"\| \*\*Self-hosted\*\* \| (.+?) \|", text)
    if selfhosted:
        if has_s5 != selfhosted.group(1).strip().startswith("yes"):
            err("L-1: Self-hosted row and §5 presence disagree")
    else:
        warn("L-1: no Self-hosted row in Notes")
    ops = re.findall(r"^### (4\.\d+) ([^\n]+)$", text, re.M)
    for num, name in ops:
        if "." in num and not re.fullmatch(r"4\.\d+", num):
            continue
        if not re.fullmatch(r"[a-z0-9]+\.[a-z]+", name):
            err(f"L-1: operation heading '{name}' is not resource.verb (lowercase, no separators)")
    for num, _ in ops:
        subs = re.findall(rf"^### {re.escape(num)}\.(\d) ", text, re.M)
        if subs != ["1", "2", "3", "4", "5"]:
            err(f"L-1: {num} subsections are {subs}, expected 1-5 in order")

    # L-2: blocks parse; python parses concatenated (helpers defined once)
    json_blocks = fences(text, "json")
    for i, b in enumerate(json_blocks):
        if b.strip().startswith('{ "func"'):
            continue
        try:
            json.loads(b)
        except json.JSONDecodeError as e:
            err(f"L-2: json block {i} does not parse: {e}")
    py_blocks = fences(text, "python")
    for i, b in enumerate(py_blocks):
        try:
            ast.parse(b)
        except SyntaxError as e:
            err(f"L-2: python block {i} does not parse: {e}")
    try:
        ast.parse("\n\n".join(py_blocks))
    except SyntaxError as e:
        err(f"L-2: python blocks do not parse concatenated: {e}")
    py = "\n".join(py_blocks)

    # L-5: rejected codes in schemas vs codes constructed in python
    schema_codes = set()
    for b in json_blocks:
        try:
            o = json.loads(b)
        except Exception:
            continue
        c = o.get("properties", {}).get("code", {})
        schema_codes |= set(c.get("enum", []))
    py_codes = set(re.findall(r'"code":\s*"([a-z_]+)"', py))
    frame_codes = {"unknown_func"}
    for c in py_codes - schema_codes - frame_codes:
        err(f"L-5: code '{c}' constructed in python but in no Rejected enum")
    for c in schema_codes - py_codes:
        warn(f"L-5: code '{c}' in a Rejected enum but never constructed in python")

    # L-6: cfg.<key> appears in the §2 table
    table_keys = set(re.findall(r"^\| `([a-z_]+)` \| `", text, re.M))
    for k in set(re.findall(r"\bcfg\.([a-z_]+)", py)):
        if k not in table_keys:
            err(f"L-6: cfg.{k} used in python but not in the §2 table")

    # L-8: raise vocabulary
    if "raise_for_status" in py:
        err("L-8: raise_for_status() is forbidden (collapses halt into release)")
    if re.search(r"^\s*except\s*:", py, re.M):
        err("L-8: bare except")
    for m in re.finditer(r'raise Exception\(\s*"([a-z]+)"', py):
        if m.group(1) not in ("halt", "release"):
            err(f'L-8: raise Exception("{m.group(1)}", ...) — only "halt"/"release" may be raised')

    # L-9: timeouts
    for m in re.finditer(r"requests\.(get|post|put|patch|delete)\(", py):
        call = py[m.start() : py.find(")", m.start()) + 1]
        # crude: look ahead a few lines for timeout=
        window = py[m.start() : m.start() + 400]
        if "timeout=" not in window.split("\n\n")[0]:
            warn(f"L-9: requests.{m.group(1)} call may lack timeout= near: {call[:60]}")

    # L-10: every sleeping python block also checks the deadline
    for i, b in enumerate(py_blocks):
        if "time.sleep" in b and "timeout_at" not in b:
            err(f"L-10: python block {i} sleeps but never checks promise.timeout_at")

    # L-11: unquoted caller values in URL paths
    for m in re.finditer(r'f"[^"]*/\{([a-z_]+)\}', py):
        var = m.group(1)
        if var in ("API", "base"):
            continue
        if not re.search(rf"{var}\s*=\s*quote\(", py):
            warn(f"L-11: f-string interpolates '{var}' into a path; is it quote(...)d?")

    # L-12: poll config iff request_poll
    monitorings = re.findall(r"\| \*\*Monitoring\*\* \| `?([a-z_]+)`? \|", text)
    if "request_poll" in monitorings and "poll" not in table_keys:
        err("L-12: request_poll operations exist but no poll key in §2")
    if "request_poll" not in monitorings and "poll" in table_keys:
        err("L-12: poll key in §2 but no request_poll operation")

    # L-13: table enums
    for m in re.findall(r"\| \*\*Invocation\*\* \| `?([a-z_]+)`? \|", text):
        if m not in ("read", "create_idempotent", "fetch_then_create", "create"):
            err(f"L-13: Invocation '{m}' not in the vocabulary")
    for m in monitorings:
        if m not in ("request_response", "request_poll"):
            err(f"L-13: Monitoring '{m}' not in the vocabulary")

    invocations = re.findall(r"\| \*\*Invocation\*\* \| `?([a-z_]+)`? \|", text)

    # L-20: a resource the plugin writes to but never reads back. Supporting
    # a resource means supporting its lifecycle — a caller who can create a
    # record and not fetch it has to leave the plugin for the other half.
    WRITE = {"create", "update", "delete", "publish", "unpublish", "archive",
             "unarchive", "run", "trigger", "retry", "stop", "merge", "import",
             "move", "comment", "cancel", "send"}
    raw_ops = re.findall(r"^### 4\.\d+ ([a-z0-9]+)\.([a-z]+)$", text, re.M)
    singulars = {r for r, _ in raw_ops}
    by_resource = {}
    for res, verb in raw_ops:
        # A batch resource is the plural of the one it batches (rows.create
        # batches row.create); it shares that resource's reads rather than
        # needing its own.
        if res.endswith("s") and res[:-1] in singulars:
            res = res[:-1]
        by_resource.setdefault(res, set()).add(verb)
    for res, verbs in sorted(by_resource.items()):
        if verbs & WRITE and not verbs & {"get", "list"}:
            warn(f"L-20: '{res}' is written by {sorted(verbs & WRITE)} but never read — "
                 f"add {res}.get/{res}.list, or say in the Param description why the "
                 f"provider offers no read")

    # L-18: a boolean the caller supplies must reach the wire as
    # "true"/"false". Python renders True/False, which providers reject.
    bool_props = set()
    for b in json_blocks:
        try:
            o = json.loads(b)
        except Exception:
            continue
        for k, v in (o.get("properties") or {}).items():
            t = v.get("type")
            if t == "boolean" or (isinstance(t, list) and "boolean" in t):
                bool_props.add(k)
    # Only args that ride in the query string are at risk: a body boolean is
    # rendered correctly by json=. §4.N.3 declares its query params in the
    # {?a,b,c = promise.param.*} notation.
    query_args = set()
    for m in re.findall(r"\{\?([^}]+?)\s*=\s*promise\.param\.\*\}", text):
        query_args |= {a.strip() for a in m.split(",")}
    at_risk = sorted(bool_props & query_args)
    if at_risk and not re.search(r'"true"\s*if|\.lower\(\)', py):
        warn(f"L-18: boolean query args {at_risk} reach the query string as Python "
             "True/False; the wire form is true/false")

    # L-19: a poll loop that begins work absorbs transient failures with a
    # consecutive-failure cap (plugin-specify, Implementation rules).
    # Only `create` leaves the external identity unrecoverable on re-entry;
    # create_idempotent and fetch_then_create can find their own work again.
    if "request_poll" in monitorings and "create" in invocations:
        if "time.sleep" in py and not re.search(r"failures\s*(\+=|=)", py):
            warn("L-19: a `create` operation polls but no loop carries a "
                 "consecutive-failure counter; its work cannot be recovered "
                 "on re-entry, so a transient failure would duplicate it")

    # L-14: Reviewed by (empty at specification time)
    rb = re.search(r"\| \*\*Reviewed by\*\* \|(.*?)\|", text)
    if rb is None:
        err("L-14: no Reviewed by row")
    elif rb.group(1).strip():
        msg = (f"L-14: Reviewed by is filled ('{rb.group(1).strip()}') — expected empty "
               "pre-review; review is a separate step by a separate agent")
        (err if pre_review else warn)(msg)

    # L-15: exactly one Documentation row per op
    doc_rows = len(re.findall(r"\| \*\*Documentation\*\* \|", text))
    expected_docs = len(ops) + 1  # per-op + §3
    if doc_rows != expected_docs:
        err(f"L-15: {doc_rows} Documentation rows, expected {expected_docs} ({len(ops)} ops + §3)")

    # L-16: prose-free
    in_fence = False
    for n, line in enumerate(text.splitlines(), 1):
        if line.startswith("```") or line.startswith("~~~"):
            in_fence = not in_fence
            continue
        if in_fence or not line.strip():
            continue
        if line.startswith(("#", "|", "**Notes**")):
            continue
        if re.fullmatch(r"Same as 4\.\d+\.\d+( .*)?\.", line.strip()) or line.strip().startswith("Same as 4."):
            continue
        err(f"L-16: prose at line {n}: {line.strip()[:70]}")

    for e in errors:
        print(f"ERROR   {e}")
    for w in warnings:
        print(f"warning {w}")
    print(f"{path}: {len(errors)} errors, {len(warnings)} warnings")
    return 1 if errors else 0


if __name__ == "__main__":
    args = sys.argv[1:]
    pre = "--pre-review" in args
    paths = [a for a in args if not a.startswith("--")]
    sys.exit(main(paths[0], pre))
