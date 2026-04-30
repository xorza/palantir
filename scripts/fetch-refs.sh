#!/usr/bin/env bash
# Clone reference UI frameworks into ./tmp for offline study.
# Re-runnable: skips repos that are already cloned, fetches updates otherwise.
# All clones are shallow (--depth 1) to save disk and bandwidth.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="$ROOT/tmp"
mkdir -p "$DEST"
cd "$DEST"

# Format: "<name> <git-url> <one-line note>"
REPOS=(
  # ---- Immediate-mode GUIs ----
  "egui|https://github.com/emilk/egui|Rust immediate-mode GUI; egui-wgpu backend; multi-pass support"
  "clay|https://github.com/nicbarker/clay|C high-performance layout lib; deferred immediate mode; arena tree"
  "imgui|https://github.com/ocornut/imgui|Dear ImGui — the canonical immediate-mode GUI (C++)"
  "nuklear|https://github.com/Immediate-Mode-UI/Nuklear|Single-header ANSI C immediate-mode GUI"

  # ---- Retained / hybrid GUIs (Rust, wgpu-friendly) ----
  "iced|https://github.com/iced-rs/iced|Rust cross-platform GUI; Elm-style; wgpu renderer"
  "xilem|https://github.com/linebender/xilem|Linebender's reactive Rust UI (Masonry/Vello/Parley)"
  "slint|https://github.com/slint-ui/slint|Declarative Rust/C++ GUI toolkit"
  "makepad|https://github.com/makepad/makepad|Rust live-coding UI; custom shader-based renderer"
  "dioxus|https://github.com/DioxusLabs/dioxus|React-like Rust UI; uses Taffy for layout"
  "druid|https://github.com/linebender/druid|Older Linebender Rust UI (archived but instructive)"
  "quirky|https://github.com/JedimEmO/quirky|Small retained-mode wgpu UI in Rust"
  "floem|https://github.com/lapce/floem|Rust UI framework used by Lapce editor"
  "freya|https://github.com/marc2332/freya|Rust GUI on top of Skia + Dioxus"

  # ---- Layout engines ----
  "taffy|https://github.com/DioxusLabs/taffy|Flex/Grid/Block layout engine in Rust"
  "morphorm|https://github.com/vizia/morphorm|One-pass layout engine used by Vizia"
  "vizia|https://github.com/vizia/vizia|Reactive Rust UI built on Morphorm"
  "yoga|https://github.com/facebook/yoga|Facebook's flexbox layout engine (C++)"
  "stretch|https://github.com/vislyhq/stretch|Predecessor of Taffy"

  # ---- Renderers / vector graphics / text ----
  "vello|https://github.com/linebender/vello|GPU compute-based 2D renderer (Rust, wgpu)"
  "lyon|https://github.com/nical/lyon|Path tessellation in Rust"
  "kurbo|https://github.com/linebender/kurbo|2D curve / path math (Rust)"
  "peniko|https://github.com/linebender/peniko|Color/brush/gradient types shared by Vello/Xilem"
  "parley|https://github.com/linebender/parley|Text layout for Vello/Xilem"
  "cosmic-text|https://github.com/pop-os/cosmic-text|Pure-Rust shaping + layout (used by glyphon)"
  "glyphon|https://github.com/grovesNL/glyphon|cosmic-text + wgpu glyph atlas"
  "wgpu|https://github.com/gfx-rs/wgpu|The GPU abstraction we render through"

  # ---- WPF (the layout model we're emulating) ----
  "wpf|https://github.com/dotnet/wpf|Windows Presentation Foundation reference source (C#)"

  # ---- Other immediate-mode / layout references ----
  "raylib|https://github.com/raysan5/raylib|C game framework; raygui is a useful immediate-mode reference"
  "bevy|https://github.com/bevyengine/bevy|bevy_ui uses Taffy; useful integration reference"
)

clone_or_update() {
  local name="$1" url="$2" note="$3"
  if [ -d "$name/.git" ]; then
    printf "  [update] %-14s %s\n" "$name" "$note"
    git -C "$name" fetch --depth 1 origin >/dev/null 2>&1 || true
    # Reset to remote HEAD; ignore failures (e.g. if default branch renamed).
    local head
    head=$(git -C "$name" remote show origin 2>/dev/null | awk '/HEAD branch/ {print $NF}') || true
    if [ -n "${head:-}" ]; then
      git -C "$name" reset --hard "origin/$head" >/dev/null 2>&1 || true
    fi
  else
    printf "  [clone]  %-14s %s\n" "$name" "$note"
    git clone --depth 1 --quiet "$url" "$name" || {
      echo "    !! failed to clone $url" >&2
      return 0
    }
  fi
}

echo "Fetching reference sources into $DEST"
echo
for entry in "${REPOS[@]}"; do
  IFS='|' read -r name url note <<< "$entry"
  clone_or_update "$name" "$url" "$note"
done

echo
echo "Done. Disk usage:"
du -sh "$DEST" 2>/dev/null || true
