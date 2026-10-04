#!/usr/bin/env python3
"""Write `.notes/API_SURFACE.md`: every item the crate exports, from rustdoc JSON.

Needs a nightly toolchain, which is what emits rustdoc JSON:

    python3 scripts/api_surface.py

Each item is rendered as its declaration — full signatures with generics,
argument and return types, public fields, variant shapes, trait items and
the trait impls with their arguments — so a re-signed item shows in the
diff, not only a renamed one.

The surface is the union over every public feature: the crate is built once
with no features and once per feature, and an item that only some builds
export is tagged with the features that bring it (or `no-default` when only
the bare build has it). `internals` and `bench` exist for this crate's own
tests and benches, and are left out.

The script also proves its own coverage: every public item defined in the
crate must be reached from the crate root by some export path. A public item
no path reaches — one in a private module, which a caller cannot name — is
listed at the end, and so is any rustdoc JSON item kind the renderer does
not know, so a missed item is reported rather than dropped.
"""

import json
import pathlib
import re
import shutil
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
JSON = ROOT / "target" / "doc" / "palantir.json"
OUT = ROOT / ".notes" / "API_SURFACE.md"
FORMAT_VERSION = 61
# Public features. `internals` and `bench` are the crate's own test surface.
FEATURES = ["winit", "system-clipboard", "golden", "gpu-debug-markers", "profile-with-tracy"]
# Traits the compiler implements, or a derive implements beside the trait it
# names; they say nothing about this crate's API. The std blanket impls
# (`From<T> for T`, `Into`, `ToOwned`, ..) are dropped by their blanket flag,
# not by name, so a real `impl<T: Num> From<T> for Size` stays.
NOISE_TRAITS = {
    "Send", "Sync", "Unpin", "UnwindSafe", "RefUnwindSafe", "Freeze",
    "StructuralPartialEq", "TrivialClone",
}
RECEIVER = re.compile(r"(&('\w+ )?(mut )?)?Self")
ITEM_KINDS = {
    "module", "struct", "enum", "union", "trait", "trait_alias", "function",
    "type_alias", "constant", "static", "macro", "proc_macro", "use",
}


def build(features):
    args = ["cargo", "+nightly", "rustdoc", "--lib", "--no-default-features"]
    if features:
        args += ["--features", ",".join(features)]
    args += ["--", "-Z", "unstable-options", "--output-format", "json",
             "--document-hidden-items"]
    subprocess.run(args, cwd=ROOT, check=True, stdout=subprocess.DEVNULL)
    doc = json.loads(JSON.read_text())
    if doc["format_version"] != FORMAT_VERSION:
        sys.exit(f"rustdoc JSON format {doc['format_version']}, this script reads "
                 f"{FORMAT_VERSION}: check the renderer against the new format")
    return doc


def anon(text):
    """`text` with every lifetime named `'_`, so `Button<'a>` matches `Button<'_>`."""
    return re.sub(r"'\w+", "'_", text)


def unbound(generics):
    """A generics list without its lifetimes: `impl Menu<'_>` elides what
    `struct Menu<'a>` declares, and narrows nothing."""
    return re.sub(r"'_(, )?", "", anon(generics)).replace("<>", "")


class Renderer:
    """Renders one build's rustdoc JSON into item blocks keyed by path."""

    def __init__(self, doc):
        self.index = doc["index"]
        self.paths = doc["paths"]
        self.root = str(doc["root"])
        self.blocks = {}
        self.prelude = []
        self.reached = set()
        self.unknown = set()

    def item(self, id_):
        return self.index.get(str(id_))

    @staticmethod
    def kind(item):
        return next(iter(item["inner"]))

    @staticmethod
    def public(item):
        return item["visibility"] == "public"

    # Types -----------------------------------------------------------------

    def ty(self, t):
        if t is None:
            return "()"
        k, v = next(iter(t.items()))
        if k == "resolved_path":
            name = v["path"].rsplit("::", 1)[-1]
            return name + self.args(v.get("args"))
        if k == "generic":
            return v
        if k == "primitive":
            return v
        if k == "borrowed_ref":
            life = f"{v['lifetime']} " if v.get("lifetime") else ""
            mut = "mut " if v["is_mutable"] else ""
            return f"&{life}{mut}{self.ty(v['type'])}"
        if k == "raw_pointer":
            return f"*{'mut' if v['is_mutable'] else 'const'} {self.ty(v['type'])}"
        if k == "tuple":
            inner = ", ".join(self.ty(x) for x in v)
            return f"({inner},)" if len(v) == 1 else f"({inner})"
        if k == "slice":
            return f"[{self.ty(v)}]"
        if k == "array":
            return f"[{self.ty(v['type'])}; {v['len']}]"
        if k == "impl_trait":
            return "impl " + self.bounds(v)
        if k == "dyn_trait":
            parts = [self.poly(tr) for tr in v["traits"]]
            if v.get("lifetime"):
                parts.append(v["lifetime"])
            return "dyn " + " + ".join(parts)
        if k == "function_pointer":
            sig = v["sig"]
            ins = ", ".join(self.ty(t) for _, t in sig["inputs"])
            out = sig.get("output")
            return f"fn({ins})" + (f" -> {self.ty(out)}" if out else "")
        if k == "qualified_path":
            trait = v.get("trait")
            # `Self::Tab` comes with the trait but an empty path.
            if not trait or not trait["path"]:
                return f"{self.ty(v['self_type'])}::{v['name']}"
            tr = trait["path"].rsplit("::", 1)[-1] + self.args(trait.get("args"))
            return f"<{self.ty(v['self_type'])} as {tr}>::{v['name']}"
        if k == "infer":
            return "_"
        if k == "pat":
            return self.ty(v["type"])
        self.unknown.add(f"type {k}")
        return f"<{k}>"

    def args(self, a):
        if not a:
            return ""
        k, v = next(iter(a.items()))
        if k == "angle_bracketed":
            parts = []
            for arg in v["args"]:
                ak, av = next(iter(arg.items()))
                parts.append(self.ty(av) if ak == "type" else (av if ak == "lifetime" else str(av)))
            for c in v.get("constraints", []):
                binding = c["binding"]
                bk, bv = next(iter(binding.items()))
                if bk == "equality":
                    tk, tv = next(iter(bv.items()))
                    parts.append(f"{c['name']} = {self.ty(tv) if tk == 'type' else tv}")
                else:
                    parts.append(f"{c['name']}: {self.bounds(bv)}")
            return f"<{', '.join(parts)}>" if parts else ""
        if k == "parenthesized":
            ins = ", ".join(self.ty(t) for t in v["inputs"])
            out = v.get("output")
            return f"({ins})" + (f" -> {self.ty(out)}" if out else "")
        if k == "return_type_notation":
            return "(..)"
        self.unknown.add(f"generic args {k}")
        return ""

    def poly(self, p):
        trait = p["trait"]
        hrtb = ""
        if p.get("generic_params"):
            hrtb = "for<" + ", ".join(g["name"] for g in p["generic_params"]) + "> "
        return hrtb + trait["path"].rsplit("::", 1)[-1] + self.args(trait.get("args"))

    def bounds(self, bounds):
        out = []
        for b in bounds:
            k, v = next(iter(b.items()))
            if k == "trait_bound":
                modifier = {"maybe": "?", "maybe_const": "~const "}.get(v.get("modifier"), "")
                out.append(modifier + self.poly(v))
            elif k == "outlives":
                out.append(v)
            elif k == "use":
                out.append("use<..>")
            else:
                self.unknown.add(f"bound {k}")
        return " + ".join(out)

    def generics(self, g):
        params = []
        for p in g["params"]:
            k, v = next(iter(p["kind"].items()))
            if k == "lifetime":
                params.append(p["name"] + (": " + " + ".join(v["outlives"]) if v["outlives"] else ""))
            elif k == "type":
                if v.get("is_synthetic"):
                    continue
                s = p["name"]
                if v["bounds"]:
                    s += ": " + self.bounds(v["bounds"])
                if v.get("default"):
                    s += " = " + self.ty(v["default"])
                params.append(s)
            elif k == "const":
                s = f"const {p['name']}: {self.ty(v['type'])}"
                if v.get("default"):
                    s += f" = {v['default']}"
                params.append(s)
        return f"<{', '.join(params)}>" if params else ""

    def where_clause(self, g):
        preds = []
        for w in g["where_predicates"]:
            k, v = next(iter(w.items()))
            if k == "bound_predicate":
                preds.append(f"{self.ty(v['type'])}: {self.bounds(v['bounds'])}")
            elif k == "lifetime_predicate":
                preds.append(f"{v['lifetime']}: {' + '.join(v['outlives'])}")
            elif k == "eq_predicate":
                preds.append(f"{self.ty(v['lhs'])} = {self.ty(v['rhs'])}")
        return f" where {', '.join(preds)}" if preds else ""

    def signature(self, name, f):
        sig, h = f["sig"], f["header"]
        prefix = "".join(w for w, on in [("const ", h["is_const"]), ("async ", h["is_async"]),
                                         ("unsafe ", h["is_unsafe"])] if on)
        ins = []
        for pname, t in sig["inputs"]:
            if pname == "self":
                shown = self.ty(t)
                # `self`, `&self`, `&'a mut self`: the receiver shorthands.
                ins.append(shown[:-4] + "self" if RECEIVER.fullmatch(shown) else f"self: {shown}")
            else:
                ins.append(f"{pname}: {self.ty(t)}")
        out = sig.get("output")
        ret = f" -> {self.ty(out)}" if out else ""
        return (f"{prefix}fn {name}{self.generics(f['generics'])}({', '.join(ins)}){ret}"
                f"{self.where_clause(f['generics'])}")

    # Items -----------------------------------------------------------------

    def attrs(self, item):
        tags = []
        for a in item.get("attrs", []):
            text = a if isinstance(a, str) else json.dumps(a)
            if "non_exhaustive" in text:
                tags.append("#[non_exhaustive]")
        return " ".join(tags)

    def walk(self, module, prefix):
        for id_ in module["inner"]["module"]["items"]:
            item = self.item(id_)
            if item is None or not self.public(item):
                continue
            self.emit(item, prefix)

    def emit(self, item, prefix, name=None):
        kind = self.kind(item)
        if kind == "use":
            use = item["inner"]["use"]
            target = self.item(use["id"]) if use["id"] is not None else None
            path = f"{prefix}{use['name']}"
            if target is None:
                self.add(path, [f"pub use {use['source']} as {use['name']}"])
            elif self.kind(target) == "module" and use["is_glob"]:
                self.reached.add(str(use["id"]))
                self.walk(target, prefix)
            else:
                self.emit(target, prefix, use["name"])
            return
        self.reached.add(str(item["id"]))
        name = name or item["name"]
        path = f"{prefix}{name}"
        inner = item["inner"][kind]
        if kind == "module":
            if name == "prelude":
                self.prelude = sorted(
                    self.item(i)["inner"]["use"]["name"]
                    for i in inner["items"]
                    if self.kind(self.item(i)) == "use"
                )
                return
            self.add(path, ["mod"])
            self.walk(item, f"{path}::")
            return
        tag = self.attrs(item)
        lines = [tag] if tag else []
        no_generics = {"params": [], "where_predicates": []}
        g = inner.get("generics", no_generics) if isinstance(inner, dict) else no_generics
        if kind == "function":
            lines.append(self.signature(name, inner))
        elif kind == "struct":
            lines.append(f"struct {name}{self.generics(g)}{self.where_clause(g)}")
            lines += self.struct_fields(inner["kind"])
            lines += self.members(name, g, inner["impls"])
        elif kind == "union":
            lines.append(f"union {name}{self.generics(g)}")
            lines += self.field_lines(inner["fields"], inner.get("has_stripped_fields"))
            lines += self.members(name, g, inner["impls"])
        elif kind == "enum":
            lines.append(f"enum {name}{self.generics(g)}{self.where_clause(g)}")
            for vid in inner["variants"]:
                lines.append("    " + self.variant(self.item(vid)))
            if inner.get("has_stripped_variants"):
                lines.append("    // and private variants")
            lines += self.members(name, g, inner["impls"])
        elif kind == "trait":
            supers = f": {self.bounds(inner['bounds'])}" if inner["bounds"] else ""
            unsafe = "unsafe " if inner["is_unsafe"] else ""
            lines.append(f"{unsafe}trait {name}{self.generics(g)}{supers}{self.where_clause(g)}")
            for iid in inner["items"]:
                lines.append("    " + self.trait_item(self.item(iid)))
            lines += self.blanket_impls(name, inner["implementations"])
        elif kind == "type_alias":
            lines.append(f"type {name}{self.generics(g)} = {self.ty(inner['type'])}")
        elif kind == "constant":
            lines.append(f"const {name}: {self.ty(inner['type'])} = {inner['const']['expr']}")
        elif kind == "static":
            mut = "mut " if inner.get("is_mutable") else ""
            lines.append(f"static {mut}{name}: {self.ty(inner['type'])}")
        elif kind == "macro":
            lines.append(f"macro {name}!")
        elif kind == "proc_macro":
            lines.append(f"proc macro {inner['kind']} {name}")
        else:
            self.unknown.add(f"item {kind}")
            lines.append(f"<{kind}> {name}")
        self.add(path, lines)

    def add(self, path, lines):
        # A path reached twice (an item and its re-export under one name)
        # keeps one block.
        self.blocks.setdefault(path, lines)

    def field_lines(self, ids, stripped):
        out = []
        for fid in ids:
            f = self.item(fid)
            if f is not None and self.public(f):
                out.append(f"    pub {f['name']}: {self.ty(f['inner']['struct_field'])}")
        if stripped:
            out.append("    // and private fields")
        return out

    def struct_fields(self, k):
        if k == "unit":
            return []
        if "plain" in k:
            return self.field_lines(k["plain"]["fields"], k["plain"]["has_stripped_fields"])
        if "tuple" in k:
            parts = []
            for fid in k["tuple"]:
                f = self.item(fid) if fid is not None else None
                parts.append(f"pub {self.ty(f['inner']['struct_field'])}"
                             if f is not None and self.public(f) else "_")
            return [f"    ({', '.join(parts)})"]
        self.unknown.add(f"struct kind {k}")
        return []

    def variant(self, v):
        inner = v["inner"]["variant"]
        k = inner["kind"]
        disc = f" = {inner['discriminant']['expr']}" if inner.get("discriminant") else ""
        if k == "plain":
            return v["name"] + disc
        if "tuple" in k:
            parts = [self.ty(self.item(f)["inner"]["struct_field"]) if f is not None else "_"
                     for f in k["tuple"]]
            return f"{v['name']}({', '.join(parts)}){disc}"
        if "struct" in k:
            fields = [f"{self.item(f)['name']}: {self.ty(self.item(f)['inner']['struct_field'])}"
                      for f in k["struct"]["fields"]]
            if k["struct"]["has_stripped_fields"]:
                fields.append("..")
            return f"{v['name']} {{ {', '.join(fields)} }}{disc}"
        self.unknown.add(f"variant kind {k}")
        return v["name"]

    def trait_item(self, it):
        k = self.kind(it)
        v = it["inner"][k]
        if k == "function":
            body = " { .. }" if v["has_body"] else ""
            return self.signature(it["name"], v) + body
        if k == "assoc_type":
            b = f": {self.bounds(v['bounds'])}" if v["bounds"] else ""
            d = f" = {self.ty(v['type'])}" if v.get("type") else ""
            return f"type {it['name']}{self.generics(v['generics'])}{b}{d}"
        if k == "assoc_const":
            d = f" = {v['value']}" if v.get("value") else ""
            return f"const {it['name']}: {self.ty(v['type'])}{d}"
        self.unknown.add(f"trait item {k}")
        return f"<{k}> {it['name']}"

    def local_trait(self, trait):
        summary = self.paths.get(str(trait["id"]))
        return summary is not None and summary["crate_id"] == 0

    def members(self, owner, g, impl_ids):
        """The inherent members and trait impls of the type `owner`.

        `impls` also holds impls that only name the type in an argument
        (`impl From<RgbaF32> for Brush`), so an impl prints its self type
        whenever that is not the owner. An inherent impl on a narrower self
        type (`impl Gradient<LinearGeometry>`) heads its own members.
        """
        plain = anon(owner + self.param_names(g))
        decl = unbound(self.generics(g))
        lines, traits, head_shown = [], set(), None
        for id_ in impl_ids:
            impl = self.item(id_)["inner"]["impl"]
            if impl["is_synthetic"]:
                continue
            trait = impl["trait"]
            for_ = self.ty(impl["for"])
            generics = self.generics(impl["generics"])
            where = self.where_clause(impl["generics"])
            if impl["blanket_impl"] is not None:
                # A std blanket impl (`impl<T> From<T> for T`) says nothing
                # about this type. This crate's own blanket impl does: it is
                # how the type gets `DockTab` or `Lower`, and the trait's
                # block shows the impl itself.
                if trait and self.local_trait(trait):
                    traits.add(f"impl {self.trait_name(trait)}  (blanket)")
                continue
            if trait is not None:
                name = self.trait_name(trait)
                if trait["path"].rsplit("::", 1)[-1] in NOISE_TRAITS:
                    continue
                neg = "!" if impl.get("is_negative") else ""
                head = f"impl{generics} {neg}{name}"
                if generics or anon(for_) != plain:
                    head += f" for {for_}"
                traits.add(head + where)
                continue
            # Members of an impl that narrows the type or bounds its
            # parameters sit under that impl's head.
            indent = "    "
            if anon(for_) != plain or unbound(generics) != decl or where:
                head = f"    impl{generics} {for_}{where}"
                if head != head_shown:
                    lines.append(head)
                    head_shown = head
                indent = "        "
            else:
                head_shown = None
            for mid in impl["items"]:
                m = self.item(mid)
                if m is None or not self.public(m):
                    continue
                mk = self.kind(m)
                if mk == "function":
                    lines.append(f"{indent}pub " + self.signature(m["name"], m["inner"]["function"]))
                elif mk == "assoc_const":
                    c = m["inner"]["assoc_const"]
                    lines.append(f"{indent}pub const {m['name']}: {self.ty(c['type'])}")
                else:
                    self.unknown.add(f"inherent item {mk}")
        lines += [f"    {t}" for t in sorted(traits)]
        return lines

    def trait_name(self, trait):
        return trait["path"].rsplit("::", 1)[-1] + self.args(trait.get("args"))

    @staticmethod
    def param_names(g):
        names = [p["name"] for p in g["params"]
                 if not next(iter(p["kind"].values())).get("is_synthetic")]
        return f"<{', '.join(names)}>" if names else ""

    def blanket_impls(self, name, implementations):
        """This trait's impls for a bare type parameter: `impl<T: ..> Tr for T`."""
        out = []
        for id_ in implementations:
            impl = self.item(id_)["inner"]["impl"]
            if "generic" in impl["for"]:
                out.append(f"    impl{self.generics(impl['generics'])} {name} for "
                           f"{self.ty(impl['for'])}{self.where_clause(impl['generics'])}")
        return out

    def render(self):
        self.walk(self.item(self.root), "")
        return self

    def unreached(self):
        """Public items defined in this crate that no export path reaches.

        Members of an impl or a trait are reached with their owner, so they
        are not counted on their own.
        """
        members = set()
        for item in self.index.values():
            k = self.kind(item)
            if k == "impl":
                members.update(str(i) for i in item["inner"]["impl"]["items"])
            elif k == "trait":
                members.update(str(i) for i in item["inner"]["trait"]["items"])
        missed = []
        for id_, item in self.index.items():
            if id_ in members:
                continue
            if item.get("crate_id") != 0 or not self.public(item):
                continue
            kind = self.kind(item)
            if kind not in ITEM_KINDS or kind in {"use", "module"}:
                continue
            if id_ in self.reached:
                continue
            summary = self.paths.get(id_)
            path = "::".join(summary["path"]) if summary else item.get("name") or id_
            # A sealed supertrait is unnameable by design: the public
            # marker over it is the API.
            if "::sealed::" in path:
                continue
            missed.append(f"{kind:<10} {path}")
        return sorted(missed)


def main():
    configs = [("no-default", [])] + [(f, [f]) for f in FEATURES]
    renders = {}
    for label, feats in configs:
        print(f"rustdoc JSON: {label}", file=sys.stderr)
        renders[label] = Renderer(build(feats)).render()
    # Leave the default build's JSON in place for anyone reading it after.
    build(["winit"])

    every = sorted(set().union(*(r.blocks for r in renders.values())))
    bare = renders["no-default"].blocks
    out_lines = []
    for path in every:
        holders = [label for label, r in renders.items() if path in r.blocks]
        block = next(renders[h].blocks[path] for h in holders)
        if "no-default" in holders and len(holders) < len(renders):
            tag = "  [no-default only: " + ", ".join(sorted(set(renders) - set(holders))) + " drop it]"
        elif "no-default" not in holders:
            tag = "  [feature: " + ", ".join(holders) + "]"
        else:
            tag = ""
        variants = {tuple(r.blocks[path]) for r in renders.values() if path in r.blocks}
        if len(variants) > 1:
            tag += "  [shape differs by feature]"
        out_lines.append(f"{path}{tag}")
        out_lines += [f"    {line}" for line in block]
        out_lines.append("")

    unreached = sorted(set().union(*(set(r.unreached()) for r in renders.values())))
    unknown = sorted(set().union(*(r.unknown for r in renders.values())))
    commit = subprocess.run(["git", "rev-parse", "--short", "HEAD"], cwd=ROOT,
                            capture_output=True, text=True).stdout.strip()
    prelude = ", ".join(f"`{n}`" for n in renders["winit"].prelude)
    text = "\n".join([
        "# Public API surface",
        "",
        "Every item the crate exports, rendered as its declaration from rustdoc JSON",
        "(`python3 scripts/api_surface.py`), sorted by path. The surface is the union of a",
        "build with no features and one build per public feature; a tag names the features an",
        "item needs. `internals` and `bench` are the crate's own test surface and are left out.",
        "",
        f"Generated on top of `{commit}` (plus the working tree).",
        "",
        f"`prelude` re-exports these root items: {prelude}.",
        "",
        "```text",
        *out_lines,
        "```",
        "",
        "## Public but not exported",
        "",
        "Items declared `pub` that no path from the crate root reaches. A caller cannot name",
        "them; each one is either dead surface or a type that leaks through a signature.",
        "",
        "```text",
        *(unreached or ["(none)"]),
        "```",
        "",
        "## Rustdoc JSON the renderer does not know",
        "",
        "```text",
        *(unknown or ["(none)"]),
        "```",
        "",
    ])
    OUT.write_text(text)
    print(f"wrote {OUT.relative_to(ROOT)}: {len(every)} items, {len(unreached)} unreached, "
          f"{len(unknown)} unknown", file=sys.stderr)


if __name__ == "__main__":
    main()
