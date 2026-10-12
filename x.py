#!/usr/bin/env python3
"""Build driver: the verification gate, focused steps, benches, and a cache warmer.

    ./x.py gate                      fmt, lint, doc, test, doctest
    ./x.py gate -p lumos             one package; --workspace for every one
    ./x.py test -p lumos -- star     the gate's test build, filtered
    ./x.py test --test alloc         target selection replaces the set's
    ./x.py nextest | lint | doc | doctest | fmt
    ./x.py bench -- '^atomic/'       every bench of this root; --smoke runs each case once
    ./x.py watch                     keeps the cache warm; --bench adds benches

Without -p or --workspace, the scope is the packages the working tree changed;
for `bench`, every package of this root with benches.

Every cargo call is one package and one step with fixed flags, built by
`Unit.argv` alone. Cargo keys an artifact on the feature set each invocation
resolves, so a call per package keeps one artifact set per package, which a
later call on the same package reuses; a flag that enters a fingerprint
(clippy's `-D warnings`, `RUSTDOCFLAGS`) overwrites the same artifact when it
changes. A crate states its sets in its own `Cargo.toml`:

    [package.metadata.x]
    lint = ["--all-features", ""]     # clippy --all-targets, once per set
    test = ["--features ml"]          # test, nextest, warm; doctest takes the first
    doc = "--all-features"
    bench = "--features wgpu"

The defaults are `["--all-features"]`, `["--all-features"]`, `"--all-features"`
and `""`. A test set may select targets (`--lib --test alloc`), `--tests`
otherwise; a `--test` target runs under the sets that enable its
`required-features`. A git submodule that holds a Cargo workspace is a root of its own:
its packages answer `-p` and change inference from here, and build in its
directory, into its target dir.
"""

from __future__ import annotations

import argparse
import enum
import json
import os
import re
import shlex
import shutil
import subprocess
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path

SCRIPT = Path(__file__).resolve()
DETAIL_LINES = 200
WATCHED_EXTS = "rs,toml,lock,wgsl,ron"
ROOT_WIDE_FILES = {
    "Cargo.toml",
    "Cargo.lock",
    "clippy.toml",
    ".clippy.toml",
    "rustfmt.toml",
    ".rustfmt.toml",
    "rust-toolchain",
    "rust-toolchain.toml",
}
PROGRESS = {
    "Adding",
    "Blocking",
    "Checking",
    "Compiling",
    "Doc-tests",
    "Documenting",
    "Downloaded",
    "Downloading",
    "Executable",
    "Finished",
    "Fresh",
    "Generated",
    "Locking",
    "Running",
    "Updating",
}
TEST_RESULT = re.compile(
    r"test result: \w+\. (\d+) passed; (\d+) failed; (\d+) ignored; \d+ measured; (\d+) filtered out"
)
QUIET_TEST_LINE = re.compile(
    r"^(test .+ \.\.\. (ok|ignored)|running \d+ tests?|test result: ok\..*|all doctests ran in .*)$"
)


class Failure(Exception):
    """A usage or project error: printed, then exit 2."""


class Step(enum.Enum):
    FMT = "fmt"
    LINT = "lint"
    SHIP = "ship"
    DOC = "doc"
    TEST = "test"
    NEXTEST = "nextest"
    DOCTEST = "doctest"
    BENCH = "bench"


@dataclass(frozen=True)
class FlagSet:
    """Feature flags, and for a test set the targets it selects."""

    features: tuple[str, ...] = ()
    targets: tuple[str, ...] = ()

    @classmethod
    def parse(cls, text: str, *, targets: bool, where: str) -> FlagSet:
        tokens = shlex.split(text)
        features: list[str] = []
        selected: list[str] = []
        i = 0
        while i < len(tokens):
            token = tokens[i]
            if token in ("--all-features", "--no-default-features") or token.startswith("--features="):
                features.append(token)
            elif token == "--features":
                if i + 1 == len(tokens):
                    raise Failure(f"{where}: `--features` needs a value")
                features += tokens[i : i + 2]
                i += 1
            elif targets and token in ("--lib", "--tests", "--bins"):
                selected.append(token)
            elif targets and token in ("--test", "--bin") and i + 1 < len(tokens):
                selected += tokens[i : i + 2]
                i += 1
            else:
                raise Failure(f"{where}: `{token}` is not a feature{' or target' if targets else ''} flag")
            i += 1
        return cls(tuple(features), tuple(selected))

    def label(self) -> str:
        return " ".join(self.features + self.targets)


@dataclass(eq=False)
class Package:
    name: str
    root: Root
    dir: Path
    lint: tuple[FlagSet, ...]
    test: tuple[FlagSet, ...]
    doc: FlagSet
    bench: FlagSet
    doctest: bool
    benches: bool
    features: dict[str, list[str]]
    test_requires: dict[str, list[str]]

    @classmethod
    def from_metadata(cls, meta: dict, root: Root) -> Package:
        where = f"{meta['manifest_path']}: [package.metadata.x]"
        config = dict((meta.get("metadata") or {}).get("x") or {})
        unknown = config.keys() - {"lint", "test", "doc", "bench"}
        if unknown:
            raise Failure(f"{where}: unknown keys {sorted(unknown)}")

        def sets(key: str, targets: bool) -> tuple[FlagSet, ...]:
            value = config.get(key, ["--all-features"])
            if not isinstance(value, list) or not value or not all(isinstance(v, str) for v in value):
                raise Failure(f"{where}: `{key}` must be a non-empty list of strings")
            return tuple(FlagSet.parse(v, targets=targets, where=f"{where}.{key}") for v in value)

        def one(key: str, default: str) -> FlagSet:
            value = config.get(key, default)
            if not isinstance(value, str):
                raise Failure(f"{where}: `{key}` must be a string")
            return FlagSet.parse(value, targets=False, where=f"{where}.{key}")

        library = ("lib", "rlib", "dylib", "proc-macro")
        return cls(
            name=meta["name"],
            root=root,
            dir=Path(meta["manifest_path"]).parent,
            lint=sets("lint", False),
            test=sets("test", True),
            doc=one("doc", "--all-features"),
            bench=one("bench", ""),
            doctest=any(t["doctest"] and set(t["kind"]) & set(library) for t in meta["targets"]),
            benches=any("bench" in t["kind"] for t in meta["targets"]),
            features=meta["features"],
            test_requires={
                t["name"]: t.get("required-features", []) for t in meta["targets"] if "test" in t["kind"]
            },
        )

    def enabled(self, flags: FlagSet) -> set[str]:
        """The package's own features a set turns on, through what each feature enables."""
        if "--all-features" in flags.features:
            return set(self.features)
        pending = [] if "--no-default-features" in flags.features else ["default"]
        for i, token in enumerate(flags.features):
            if token == "--features":
                pending += re.split(r"[ ,]+", flags.features[i + 1])
            elif token.startswith("--features="):
                pending += re.split(r"[ ,]+", token.removeprefix("--features="))
        on: set[str] = set()
        while pending:
            name = pending.pop()
            if name in self.features and name not in on:
                on.add(name)
                pending += self.features[name]
        return on

    def runs(self, flags: FlagSet) -> bool:
        """Whether every `--test` target of the set has the features it requires."""
        names = [flags.targets[i + 1] for i, t in enumerate(flags.targets) if t == "--test"]
        on = self.enabled(flags)
        return all(set(self.test_requires.get(name, [])) <= on for name in names)


@dataclass(eq=False)
class Root:
    """A Cargo workspace: one directory, one lockfile, one target dir."""

    dir: Path
    target: Path
    packages: list[Package] = field(default_factory=list)

    @classmethod
    def load(cls, path: Path) -> list[Root]:
        """This root, then every root nested in it through git submodules."""
        out = subprocess.run(
            ["cargo", "metadata", "--no-deps", "--format-version", "1"],
            cwd=path,
            capture_output=True,
            encoding="utf-8",
        )
        if out.returncode != 0:
            raise Failure(f"cargo metadata in {path}:\n{out.stderr.strip()}")
        meta = json.loads(out.stdout)
        root = cls(path, Path(meta["target_directory"]))
        members = set(meta["workspace_members"])
        root.packages = [Package.from_metadata(p, root) for p in meta["packages"] if p["id"] in members]
        member_dirs = {p.dir for p in root.packages}
        roots = [root]
        for sub in root.submodules():
            if (sub / "Cargo.toml").is_file() and sub not in member_dirs:
                roots += cls.load(sub)
        return roots

    def submodules(self) -> list[Path]:
        if not (self.dir / ".gitmodules").is_file():
            return []
        out = subprocess.run(
            ["git", "config", "--file", ".gitmodules", "--get-regexp", r"^submodule\..*\.path$"],
            cwd=self.dir,
            capture_output=True,
            encoding="utf-8",
        )
        return [self.dir / line.split(" ", 1)[1] for line in out.stdout.splitlines() if " " in line]

    def changed_paths(self) -> list[Path]:
        out = subprocess.run(
            ["git", "status", "--porcelain=v1", "-z", "--untracked-files=all"],
            cwd=self.dir,
            capture_output=True,
            encoding="utf-8",
        )
        if out.returncode != 0:
            raise Failure(f"git status in {self.dir}:\n{out.stderr.strip()}")
        # A rename or copy names its source in the next entry, bare, and both
        # packages change.
        paths = []
        rename_source = False
        for entry in out.stdout.split("\0"):
            if entry:
                paths.append(self.dir / (entry if rename_source else entry[3:]))
                rename_source = not rename_source and entry[0] in "RC"
        return paths


@dataclass
class Workspace:
    roots: list[Root]

    def __post_init__(self) -> None:
        names = [p.name for p in self.packages]
        twice = sorted({n for n in names if names.count(n) > 1})
        if twice:
            raise Failure(f"packages named in two roots: {', '.join(twice)}")

    @property
    def top(self) -> Root:
        return self.roots[0]

    @property
    def packages(self) -> list[Package]:
        return [p for r in self.roots for p in r.packages]

    def package(self, name: str) -> Package:
        for p in self.packages:
            if p.name == name:
                return p
        raise Failure(f"no package `{name}`; known: {', '.join(p.name for p in self.packages)}")

    def owners(self, paths: list[Path]) -> list[Package]:
        """The packages whose build a change to these paths can alter, in workspace order."""
        hit: set[Package] = set()
        roots = sorted(self.roots, key=lambda r: len(r.dir.parts), reverse=True)
        for path in paths:
            root = next((r for r in roots if path.is_relative_to(r.dir)), None)
            if root is None:
                continue
            rel = path.relative_to(root.dir)
            # A submodule's own entry in its parent: its root reports its files.
            if not rel.parts:
                continue
            if rel.parts[0] == ".cargo" or str(rel) in ROOT_WIDE_FILES:
                hit.update(root.packages)
                continue
            if any(part.startswith(".") for part in rel.parts):
                continue
            owner = max(
                (p for p in root.packages if path.is_relative_to(p.dir)),
                key=lambda p: len(p.dir.parts),
                default=None,
            )
            if owner is not None:
                hit.add(owner)
        return [p for p in self.packages if p in hit]

    def changed(self) -> list[Package]:
        return self.owners([path for r in self.roots for path in r.changed_paths()])


@dataclass(frozen=True)
class Session:
    """Options of the whole run, which no fingerprint reads."""

    color: bool = False
    locked: bool = False


@dataclass(frozen=True)
class Unit:
    step: Step
    package: Package
    flags: FlagSet = FlagSet()
    extra: tuple[str, ...] = ()
    build_only: bool = False
    check: bool = False

    def argv(self, session: Session) -> list[str]:
        p = self.package
        features = list(self.flags.features)
        targets = list(self.flags.targets) or ["--tests"]
        fmt = "json-diagnostic-rendered-ansi" if session.color else "json"
        cargo = ["cargo", "--locked"] if session.locked else ["cargo"]
        message = ["--message-format", fmt]
        no_run = ["--no-run"] if self.build_only else []
        extra = ["--", *self.extra] if self.extra else []
        match self.step:
            case Step.FMT:
                check = ["--check"] if self.check else []
                return ["cargo", "fmt", "-p", p.name, *check, "--", "--files-with-diff"]
            case Step.LINT:
                return [*cargo, "clippy", "-p", p.name, "--all-targets", *features, *message, "--", "-D", "warnings"]
            case Step.SHIP:
                return [*cargo, "clippy", "-p", p.name, *message, "--", "-D", "warnings"]
            case Step.DOC:
                return [*cargo, "doc", "-p", p.name, "--no-deps", "--document-private-items", *features, *message]
            case Step.TEST:
                return [*cargo, "test", "-p", p.name, *features, *targets, *no_run, *message, *extra]
            case Step.NEXTEST:
                return [
                    *cargo, "nextest", "run", "-p", p.name, *features, *targets,
                    "--status-level", "slow", "--final-status-level", "slow", "--hide-progress-bar",
                    "--color", "always" if session.color else "never", "--cargo-message-format", fmt, *extra,
                ]  # fmt: skip
            case Step.DOCTEST:
                return [*cargo, "test", "-p", p.name, "--doc", *features, *message, *extra]
            case Step.BENCH:
                build = [*no_run, *message] if self.build_only else []
                return [*cargo, "bench", "-p", p.name, "--benches", *features, *build, *extra]

    def label(self) -> str:
        if self.step is Step.FMT:
            return ""
        return self.flags.label() or "default features"

    def env(self) -> dict[str, str]:
        env = dict(os.environ)
        if self.step is Step.DOC:
            env["RUSTDOCFLAGS"] = "-D warnings"
        else:
            env.pop("RUSTDOCFLAGS", None)
        return env

    @property
    def streams(self) -> bool:
        """A bench prints its own progress, and runs too long to stay silent."""
        return self.step is Step.BENCH and not self.build_only


@dataclass
class Outcome:
    ok: bool
    seconds: float
    built: bool = False
    summary: str = ""
    diagnostics: list[str] = field(default_factory=list)
    detail: list[str] = field(default_factory=list)
    log: Path | None = None


class Console:
    def __init__(self, units: list[Unit]):
        self.tty = sys.stdout.isatty()
        self.color = self.tty and "NO_COLOR" not in os.environ
        self.step_width = max((len(u.step.value) for u in units), default=0)
        self.name_width = max((len(u.package.name) for u in units), default=0)
        self.shown: set[str] = set()

    def phase(self) -> None:
        """Units of one phase often fail on the same diagnostic; it prints once per phase."""
        self.shown.clear()

    def paint(self, text: str, code: str) -> str:
        return f"\x1b[{code}m{text}\x1b[0m" if self.color else text

    def status(self, outcome: Outcome) -> str:
        return self.paint("ok  ", "32") if outcome.ok else self.paint("FAIL", "1;31")

    def row(self, status: str, unit: Unit, seconds: str, note: str = "") -> str:
        label = unit.label()
        cells = [
            status,
            seconds.rjust(6),
            unit.step.value.ljust(self.step_width),
            unit.package.name.ljust(self.name_width),
        ]
        line = "  ".join(cells)
        if label:
            line += "  " + self.paint(label, "2")
        if note:
            line += "  " + note
        return line.rstrip()

    def start(self, unit: Unit) -> None:
        if self.tty and not unit.streams:
            print(self.row(self.paint("..  ", "2"), unit, ""), end="", flush=True)
        elif unit.streams:
            print(self.row("    ", unit, ""), flush=True)

    def finish(self, unit: Unit, outcome: Outcome) -> None:
        if self.tty and not unit.streams:
            print("\r\x1b[K", end="")
        print(self.row(self.status(outcome), unit, duration(outcome.seconds), outcome.summary), flush=True)
        if outcome.ok:
            return
        fresh = [d for d in outcome.diagnostics if d not in self.shown]
        self.shown.update(fresh)
        lines = [line for d in fresh for line in d.rstrip().splitlines()] + outcome.detail
        if len(lines) > DETAIL_LINES:
            lines = lines[:DETAIL_LINES] + [f"... {len(lines) - DETAIL_LINES} more lines"]
        repeated = len(outcome.diagnostics) - len(fresh)
        if repeated:
            lines.append(f"({repeated} diagnostic{'s' * (repeated > 1)} shown above)")
        if outcome.log is not None:
            lines.append(f"log: {outcome.log}")
        for line in lines:
            print("    " + line)
        sys.stdout.flush()


def duration(seconds: float) -> str:
    if seconds < 60:
        return f"{seconds:.1f}s"
    minutes, rest = divmod(round(seconds), 60)
    return f"{minutes}m{rest:02d}s"


def run(unit: Unit, log: Path, session: Session) -> Outcome:
    argv = unit.argv(session)
    started = time.monotonic()
    if unit.streams:
        code = subprocess.run(argv, cwd=unit.package.root.dir, env=unit.env()).returncode
        return Outcome(code == 0, time.monotonic() - started)
    proc = subprocess.run(
        argv,
        cwd=unit.package.root.dir,
        env=unit.env(),
        capture_output=True,
        encoding="utf-8",
        errors="replace",
    )
    seconds = time.monotonic() - started
    log.parent.mkdir(parents=True, exist_ok=True)
    log.write_text(f"$ {shlex.join(argv)}\n\n{proc.stdout}\n{proc.stderr}", encoding="utf-8")
    outcome = Outcome(proc.returncode == 0, seconds, log=log)
    read_output(unit, proc.stdout, proc.stderr, outcome)
    return outcome


def read_output(unit: Unit, stdout: str, stderr: str, outcome: Outcome) -> None:
    plain: list[str] = []
    executables: list[str] = []
    for line in stdout.splitlines():
        message = json.loads(line) if line.startswith('{"reason":') else None
        if message is None:
            plain.append(line)
        elif message["reason"] == "compiler-artifact":
            outcome.built |= not message["fresh"]
            if "bench" in message["target"]["kind"] and message["executable"]:
                executables.append(message["executable"])
        elif message["reason"] == "compiler-message":
            rendered = diagnostic(message["message"], warnings=unit.step in (Step.LINT, Step.SHIP, Step.DOC))
            if rendered is not None and rendered not in outcome.diagnostics:
                outcome.diagnostics.append(rendered)
    errors = [line for line in stderr.splitlines() if not is_progress(line)]

    if unit.step is Step.FMT:
        files = [os.path.relpath(f, unit.package.root.dir) for f in plain if f.strip()]
        if files:
            outcome.summary = ("unformatted " if unit.check else "rewrote ") + ", ".join(files)
    elif unit.step is Step.BENCH:
        outcome.summary = ", ".join(os.path.relpath(e, unit.package.root.dir) for e in executables)
    elif unit.step in (Step.TEST, Step.DOCTEST) and not unit.build_only:
        outcome.summary = test_summary(plain)
    elif unit.step is Step.NEXTEST:
        summary = next((line for line in stderr.splitlines() if line.strip().startswith("Summary")), "")
        outcome.summary = re.sub(r"^\s*Summary \[[^]]*\]\s*", "", summary)
    if outcome.ok:
        return
    if unit.step is Step.FMT:
        outcome.detail = errors
        return
    outcome.detail = [line for line in plain if not QUIET_TEST_LINE.match(line) and line.strip()] + errors


def diagnostic(message: dict, *, warnings: bool) -> str | None:
    """A warning fails only the steps that deny warnings; elsewhere it hides the failure."""
    levels = ("error", "warning") if warnings else ("error",)
    if message["level"] not in levels or message["rendered"] is None:
        return None
    text = message["message"]
    if not message["spans"] and (
        text.startswith("aborting due to") or re.fullmatch(r"\d+ warnings? emitted", text)
    ):
        return None
    return message["rendered"]


def is_progress(line: str) -> bool:
    words = line.split()
    if not words:
        return True
    if words[0] in PROGRESS:
        return True
    return words[0] == "warning:" and bool(re.search(r"generated \d+ warnings?|build failed, waiting", line))


def test_summary(lines: list[str]) -> str:
    totals = [0, 0, 0, 0]
    for line in lines:
        found = TEST_RESULT.search(line)
        if found:
            totals = [a + int(b) for a, b in zip(totals, found.groups())]
    passed, failed, ignored, filtered = totals
    parts = [f"{passed} passed"]
    for count, word in ((failed, "failed"), (ignored, "ignored"), (filtered, "filtered out")):
        if count:
            parts.append(f"{count} {word}")
    return ", ".join(parts)


class Logs:
    """One file per unit under `target/x/<kind>/`, numbered after a package's first set."""

    def __init__(self, kind: str):
        self.kind = kind
        self.counts: dict[tuple[str, Step], int] = {}

    def path(self, unit: Unit) -> Path:
        key = (unit.package.name, unit.step)
        index = self.counts.get(key, 0)
        self.counts[key] = index + 1
        name = f"{unit.package.name}.{unit.step.value}{f'.{index}' if index else ''}.log"
        return unit.package.root.target / "x" / self.kind / name


@dataclass(frozen=True)
class Selection:
    """What a command adds to the crate's own sets: target selection and pass-through args."""

    extra: tuple[str, ...] = ()
    targets: tuple[str, ...] = ()
    smoke: bool = False
    build_only: bool = False
    check: bool = False

    @classmethod
    def from_args(cls, args: argparse.Namespace) -> Selection:
        targets = ["--lib"] if getattr(args, "lib", False) else []
        for name in getattr(args, "test", None) or ():
            targets += ["--test", name]
        return cls(
            extra=tuple(args.extra),
            targets=tuple(targets),
            smoke=getattr(args, "smoke", False),
            build_only=getattr(args, "no_run", False),
            check=getattr(args, "check", False),
        )

    def units(self, step: Step, packages: list[Package]) -> list[Unit]:
        out: list[Unit] = []
        for p in packages:
            match step:
                case Step.FMT:
                    out.append(Unit(step, p, check=self.check))
                case Step.SHIP:
                    out.append(Unit(step, p))
                case Step.LINT:
                    out += [Unit(step, p, s) for s in p.lint]
                case Step.DOC:
                    out.append(Unit(step, p, p.doc))
                case Step.TEST | Step.NEXTEST:
                    sets = [FlagSet(s.features, self.targets) if self.targets else s for s in p.test]
                    sets = [s for s in sets if p.runs(s)]
                    if not sets:
                        raise Failure(f"no test set of {p.name} has the features {' '.join(self.targets)} requires")
                    out += [Unit(step, p, s, self.extra, self.build_only) for s in sets]
                case Step.DOCTEST:
                    if p.doctest:
                        out.append(Unit(step, p, FlagSet(p.test[0].features), self.extra))
                case Step.BENCH:
                    if p.benches:
                        smoke = ("--test",) if self.smoke else ()
                        out.append(Unit(step, p, p.bench, self.extra + smoke, self.build_only))
        return list(dict.fromkeys(out))


def run_phases(phases: list[list[Unit]], locked: bool) -> int:
    console = Console([u for phase in phases for u in phase])
    session = Session(console.color, locked)
    logs = Logs("last")
    for i, phase in enumerate(phases):
        console.phase()
        failed = False
        for unit in phase:
            console.start(unit)
            outcome = run(unit, logs.path(unit), session)
            console.finish(unit, outcome)
            failed |= not outcome.ok
        if failed:
            skipped = list(dict.fromkeys(u.step.value for later in phases[i + 1 :] for u in later))
            if skipped:
                print(f"stopped: skipped {', '.join(skipped)}", flush=True)
            return 1
    return 0


PHASES = {
    "gate": [[Step.FMT], [Step.LINT, Step.SHIP], [Step.DOC], [Step.TEST], [Step.DOCTEST]],
    "fmt": [[Step.FMT]],
    "lint": [[Step.LINT, Step.SHIP]],
    "doc": [[Step.DOC]],
    "test": [[Step.TEST]],
    "nextest": [[Step.NEXTEST]],
    "doctest": [[Step.DOCTEST]],
    "bench": [[Step.BENCH]],
}
TAKES_EXTRA = {"test", "nextest", "doctest", "bench"}


def scope(ws: Workspace, args: argparse.Namespace) -> list[Package]:
    """Benches measure code whether it changed or not, so they take the whole root by default."""
    if args.package:
        packages = [ws.package(name) for name in dict.fromkeys(args.package)]
        how = ""
    elif args.workspace or args.command == "bench":
        packages = list(ws.top.packages)
        how = " (workspace)"
    else:
        packages = ws.changed()
        if not packages:
            raise Failure("no package changed; name one with -p, or use --workspace")
        how = " (changed)"
    if args.command == "bench":
        packages = [p for p in packages if p.benches]
        if not packages:
            raise Failure("no package in scope has benches")
    print(f"scope: {', '.join(p.name for p in packages)}{how}", flush=True)
    return packages


def command(ws: Workspace, args: argparse.Namespace) -> int:
    packages = scope(ws, args)
    selection = Selection.from_args(args)
    phases = [[u for step in steps for u in selection.units(step, packages)] for steps in PHASES[args.command]]
    return run_phases([phase for phase in phases if phase], args.locked)


def warm(ws: Workspace, args: argparse.Namespace) -> int:
    """Builds what the gate builds, changed packages first, at the lowest priority."""
    lower_priority()
    events = "" if sys.stdin is None or sys.stdin.isatty() else sys.stdin.read()
    paths = [Path(line.split(":", 1)[1]) for line in events.splitlines() if ":" in line]
    changed = ws.owners(paths)
    rest = [p for p in ws.packages if p not in changed]
    steps = [Step.LINT, Step.SHIP, Step.DOC, Step.TEST] + ([Step.BENCH] if args.bench else [])
    build = Selection(build_only=True)
    units = [u for group in (changed, rest) for step in steps for u in build.units(step, group)]

    console = Console(units)
    started = time.monotonic()
    stamp = time.strftime("%H:%M:%S")
    print(console.paint(f"{stamp} warm · changed: {', '.join(p.name for p in changed) or '-'}", "2"), flush=True)
    built = failed = 0
    logs = Logs("warm")
    for unit in units:
        outcome = run(unit, logs.path(unit), Session())
        built += outcome.built
        failed += not outcome.ok
        if outcome.built or not outcome.ok:
            note = "" if outcome.ok else f"log: {outcome.log}"
            print(console.row(console.status(outcome), unit, duration(outcome.seconds), note), flush=True)
    summary = f"{stamp} warm done in {duration(time.monotonic() - started)} · {built} built · {failed} failed"
    print(console.paint(summary, "2"), flush=True)
    return 0


def lower_priority() -> None:
    """Agents' own builds get the CPU first; the children inherit the priority."""
    if os.name == "nt":
        import ctypes

        idle_priority_class = 0x40
        kernel32 = ctypes.windll.kernel32
        kernel32.SetPriorityClass(kernel32.GetCurrentProcess(), idle_priority_class)
    else:
        os.nice(19)


def watch(ws: Workspace, args: argparse.Namespace) -> int:
    watchexec = shutil.which("watchexec")
    if watchexec is None:
        raise Failure("`watch` needs watchexec on PATH")
    argv = [
        watchexec,
        "--quiet",
        "--on-busy-update=restart",
        "--debounce=1s",
        "--emit-events-to=stdio",
        "--exts", WATCHED_EXTS,
        "--ignore", "**/target/**",
        "--",
        sys.executable, str(SCRIPT), "warm", *(["--bench"] if args.bench else []),
    ]  # fmt: skip
    return subprocess.run(argv, cwd=ws.top.dir).returncode


def parser() -> argparse.ArgumentParser:
    top = argparse.ArgumentParser(prog="x.py", description=__doc__.split("\n", 1)[0])
    top.add_argument("--locked", action="store_true", help="pass --locked to cargo, as CI does")
    commands = top.add_subparsers(dest="command", required=True, metavar="command")
    scoped = argparse.ArgumentParser(add_help=False)
    group = scoped.add_mutually_exclusive_group()
    group.add_argument("-p", "--package", action="append", default=[], help="a package, by name; repeatable")
    group.add_argument("--workspace", action="store_true", help="every package of this root")
    targets = argparse.ArgumentParser(add_help=False)
    targets.add_argument("--lib", action="store_true", help="only the library's tests")
    targets.add_argument("--test", action="append", metavar="NAME", help="an integration test; repeatable")

    commands.add_parser("gate", parents=[scoped], help="fmt, lint, doc, test, doctest; stops after a failed step")
    fmt = commands.add_parser("fmt", parents=[scoped], help="rustfmt, written in place")
    fmt.add_argument("--check", action="store_true", help="list unformatted files and fail, as CI does")
    commands.add_parser("lint", parents=[scoped], help="clippy on every target per lint set, then the shipped build")
    commands.add_parser("doc", parents=[scoped], help="rustdoc with private items, warnings denied")
    commands.add_parser("test", parents=[scoped, targets], help="cargo test per test set; -- passes test args")
    commands.add_parser("nextest", parents=[scoped, targets], help="cargo nextest per test set")
    commands.add_parser("doctest", parents=[scoped], help="doc tests, under the first test set")
    bench = commands.add_parser("bench", parents=[scoped], help="criterion benches; -- passes a filter")
    bench.add_argument("--smoke", action="store_true", help="run each case once")
    bench.add_argument("--no-run", action="store_true", help="build and print the bench binaries")
    for name, text in (("watch", "rebuild on change with watchexec"), ("warm", "one build of what watch keeps warm")):
        sub = commands.add_parser(name, help=text)
        sub.add_argument("--bench", action="store_true", help="also build the benches")
    return top


def main() -> int:
    # Cargo and rustc speak UTF-8; a Windows pipe would take the ANSI code page.
    for stream in (sys.stdout, sys.stderr):
        stream.reconfigure(encoding="utf-8", errors="replace")
    argv = sys.argv[1:]
    extra: list[str] = []
    if "--" in argv:
        split = argv.index("--")
        argv, extra = argv[:split], argv[split + 1 :]
    args = parser().parse_args(argv)
    args.extra = extra
    try:
        if extra and args.command not in TAKES_EXTRA:
            raise Failure(f"`{args.command}` takes no arguments after --")
        for name in ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS"):
            if name in os.environ:
                print(
                    f"warning: {name} replaces the rustflags of .cargo/config.toml, "
                    "so this build shares no artifacts with a build without it",
                    file=sys.stderr,
                )
        ws = Workspace(Root.load(SCRIPT.parent))
        if args.command == "watch":
            return watch(ws, args)
        if args.command == "warm":
            return warm(ws, args)
        return command(ws, args)
    except Failure as failure:
        print(f"error: {failure}", file=sys.stderr)
        return 2
    except KeyboardInterrupt:
        return 130


if __name__ == "__main__":
    sys.exit(main())
