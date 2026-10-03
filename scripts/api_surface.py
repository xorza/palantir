#!/usr/bin/env python3
"""Write `.notes/API_SURFACE.md`: every item the crate exports, from rustdoc JSON.

Needs a nightly toolchain, which is what emits rustdoc JSON:

    python3 scripts/api_surface.py

The inventory covers the default features plus `golden`. `internals` and
`bench` are left out: they exist for this crate's own tests and benches.
"""

import json
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
FEATURES = "golden,winit"
JSON = ROOT / "target" / "doc" / "palantir.json"
OUT = ROOT / ".notes" / "API_SURFACE.md"
# The auto traits and blanket impls every type has say nothing about the API.
SKIPPED_TRAITS = {"Send", "Sync", "Unpin", "UnwindSafe", "RefUnwindSafe", "Freeze"}


def build():
    subprocess.run(
        [
            "cargo", "+nightly", "rustdoc", "--lib", "--features", FEATURES,
            "--", "-Z", "unstable-options", "--output-format", "json",
        ],
        cwd=ROOT,
        check=True,
    )
    return json.loads(JSON.read_text())


class Surface:
    def __init__(self, doc):
        self.index = doc["index"]
        self.paths = doc["paths"]
        self.root = str(doc["root"])
        self.lines = []
        self.prelude = []

    def item(self, id_):
        return self.index.get(str(id_))

    @staticmethod
    def kind(item):
        return next(iter(item["inner"]))

    @staticmethod
    def public(item):
        return item["visibility"] == "public"

    def walk_module(self, module, prefix):
        for id_ in module["inner"]["module"]["items"]:
            item = self.item(id_)
            if item is None or not self.public(item):
                continue
            self.emit(item, prefix)

    def emit(self, item, prefix, name=None):
        kind = self.kind(item)
        if kind == "use":
            name = name or item["inner"]["use"]["name"]
        name = name or item["name"]
        path = f"{prefix}{name}"
        if kind == "use":
            use = item["inner"]["use"]
            target = self.item(use["id"]) if use["id"] is not None else None
            if target is None:
                self.lines.append(f"{'extern-reexport':<16} {path}  -> {use['source']}")
            elif self.kind(target) == "module" and use["is_glob"]:
                self.walk_module(target, prefix)
            else:
                self.emit(target, prefix, use["name"])
            return
        if kind == "module":
            if name == "prelude":
                self.prelude = sorted(
                    self.item(i)["inner"]["use"]["name"]
                    for i in item["inner"]["module"]["items"]
                    if self.kind(self.item(i)) == "use"
                )
                return
            self.lines.append(f"{'module':<16} {path}")
            self.walk_module(item, f"{path}::")
            return
        label = {"type_alias": "type_alias", "assoc_const": "constant"}.get(kind, kind)
        if kind == "function":
            self.lines.append(f"{label:<16} {path}{self.signature(item)}")
            return
        self.lines.append(f"{label:<16} {path}")
        inner = item["inner"][kind]
        if kind == "struct":
            plain = inner["kind"].get("plain") if isinstance(inner["kind"], dict) else None
            if plain:
                fields = [
                    self.item(f)["name"]
                    for f in plain["fields"]
                    if self.public(self.item(f))
                ]
                if fields:
                    self.lines.append(f"    fields: {', '.join(fields)}")
            self.members(inner["impls"])
        elif kind == "enum":
            variants = [self.item(v)["name"] for v in inner["variants"]]
            self.lines.append(f"    variants: {', '.join(variants)}")
            self.members(inner["impls"])
        elif kind == "union":
            self.members(inner["impls"])
        elif kind == "trait":
            names = [self.item(i)["name"] for i in inner["items"]]
            self.lines.append(f"    items: {', '.join(names)}")

    def signature(self, function):
        sig = function["inner"]["function"]["sig"]
        params = ", ".join(name for name, _ in sig["inputs"])
        return f"({params})"

    def members(self, impl_ids):
        traits = set()
        for id_ in impl_ids:
            impl = self.item(id_)["inner"]["impl"]
            if impl["is_synthetic"] or impl["blanket_impl"] is not None:
                continue
            if impl["trait"] is not None:
                name = impl["trait"]["path"].rsplit("::", 1)[-1]
                if name not in SKIPPED_TRAITS:
                    traits.add(name)
                continue
            for member_id in impl["items"]:
                member = self.item(member_id)
                if not self.public(member):
                    continue
                kind = self.kind(member)
                if kind == "assoc_const":
                    self.lines.append(f"    assoc_const {member['name']}")
                elif kind == "function":
                    header = member["inner"]["function"]["header"]
                    keyword = "const fn" if header["is_const"] else "fn"
                    self.lines.append(f"    {keyword} {member['name']}{self.signature(member)}")
        if traits:
            self.lines.append(f"    traits: {', '.join(sorted(traits))}")

    def render(self):
        root = self.item(self.root)
        self.walk_module(root, "")
        commit = subprocess.run(
            ["git", "rev-parse", "--short", "HEAD"], cwd=ROOT, capture_output=True, text=True
        ).stdout.strip()
        prelude = ", ".join(f"`{name}`" for name in self.prelude)
        return "\n".join(
            [
                "# Public API surface",
                "",
                "Every item the crate exports with the default features plus `golden`, from rustdoc JSON",
                "(`python3 scripts/api_surface.py`). Each type lists its public inherent methods and",
                "associated constants, and the traits it implements. `internals` and `bench` are left out:",
                "they exist for this crate's own tests and benches.",
                "",
                f"Generated on top of `{commit}`. Findings and recommendations are in `API_CHANGES.md`.",
                "",
                f"`prelude` re-exports these root items: {prelude}.",
                "",
                "```text",
                *self.lines,
                "```",
                "",
            ]
        )


def main():
    surface = Surface(build())
    OUT.write_text(surface.render())
    print(f"wrote {OUT.relative_to(ROOT)}: {len(surface.lines)} lines", file=sys.stderr)


if __name__ == "__main__":
    main()
