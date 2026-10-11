#!/usr/bin/env python3
"""Check one translation against English: scripts/check-locale.py de
Same keys, same %{...} placeholders, valid JSON. Prints problems; exit 1 if any."""
import json, pathlib, re, sys

root = pathlib.Path(__file__).resolve().parent.parent / "locales"
code = sys.argv[1]
en = json.loads((root / "en.json").read_text())
try:
    tr = json.loads((root / f"{code}.json").read_text())
except Exception as e:
    sys.exit(f"{code}.json: {e}")
ph = lambda s: sorted(re.findall(r"%\{\w+\}", s))
problems = []
for k in en:
    if k.startswith("_"):
        continue
    if k not in tr:
        problems.append(f"missing: {k!r}")
    elif not isinstance(tr[k], str):
        problems.append(f"not a string: {k!r}")
    elif ph(tr[k]) != ph(k):
        problems.append(f"placeholders differ: {k!r} -> {tr[k]!r}")
problems += [f"extra key: {k!r}" for k in tr if not k.startswith("_") and k not in en]
same = [k for k in en if not k.startswith("_") and tr.get(k) == k and re.search(r"[a-z]{3}", k)]
for p in problems:
    print(p)
print(f"{code}: {len(en) - 1} keys, {len(problems)} problems, {len(same)} values left identical to English")
sys.exit(1 if problems else 0)
