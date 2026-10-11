#!/usr/bin/env python3
"""Collect every t!("...") key in src/ into locales/en.json (English: the key
is the text). Run after adding or changing user-visible strings; the test
`i18n::tests::every_key_is_in_en_json` fails until you do."""
import json, pathlib, re

root = pathlib.Path(__file__).resolve().parent.parent
# t!( but not format!( or print!(.
pattern = re.compile(r'(?:^|[^A-Za-z0-9_])t!\(\s*"((?:[^"\\]|\\.)*)"', re.M)
keys = set()
for f in sorted((root / "src").rglob("*.rs")):
    for m in pattern.finditer(f.read_text()):
        keys.add(json.loads(f'"{m.group(1)}"'))
out = {"_version": 1}
out.update({k: k for k in sorted(keys)})
path = root / "locales" / "en.json"
path.write_text(json.dumps(out, ensure_ascii=False, indent=1) + "\n")
print(f"{len(keys)} strings -> {path.relative_to(root)}")
