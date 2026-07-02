# RawBlow

[한국어](README.md) · **English** · [日本語](README.ja.md)

[![Downloads](https://img.shields.io/github/downloads/ascoeur9/rawblow/total?label=downloads&color=brightgreen)](https://github.com/ascoeur9/rawblow/releases)
[![Latest release](https://img.shields.io/github/v/release/ascoeur9/rawblow?sort=semver&label=latest)](https://github.com/ascoeur9/rawblow/releases/latest)

**Fast RAW photo culling (selection) viewer** — Rust + egui/wgpu native · Windows / macOS / Linux

A lightweight culling tool that lets you flip quickly through thousands of RAWs, sort them into **Pick / Hold / Reject**, and then copy or move only the ones you chose. It instantly displays the embedded preview inside the RAW for fast flipping, and loads the full resolution at native size with **ORIG (original)** only when you need to inspect focus and detail precisely. Sorting results are written only to a sidecar file without ever touching the originals (non-destructive).

---

## Key features

- **Two views** — Single view (+ bottom film strip) / thumbnail grid (toggle with T)
- **Sorting** — Q Pick · W Hold · E Reject · R Clear, auto-advance, filter by label
- **Star rating (★1–5)** — Rate with the number keys **1–5**, clear with **`` ` `` (backtick)**. Ratings are applied **independently** of labels (QWER) at the same time, and you can also select by rating when transferring
- **Color tags** — Assign 5-color tags (orange, pink, teal, blue, purple) with **⇧1–5**, **independently** of labels and ratings. Name each color in settings (e.g., by edit style), reflected in filters, transfers, and file names
- **View filters** — In the left rail, combine **label**, **rating (exactly N stars)**, and **color tag** **independently with AND** to narrow down to just the shots you want
- **Multi-select in grid** — Toggle with Ctrl(⌘)+click, click then Shift+click for range selection → sort them **all at once** with Q/W/E/R / ratings
- **AI culling** — Analyzes photos to auto-sort by **blur (focus) · exposure · horizon tilt**, plus an optional **aesthetic score (CLIP-IQA AI)**. Assigns good/reject to one of **label · rating · color tag** (your manual axes stay untouched). Fully **local** and **GPU-accelerated** (Windows WebGPU / macOS CoreML), runs in the background. The model is downloaded automatically on first use (sha256-verified)
- **Zoom / pan** — Click a photo to switch fit-to-window↔1:1, **Ctrl+wheel or touchpad pinch** for continuous zoom, **drag to pan** while zoomed in. The zoom level is shown at the bottom right
- **ORIG (original view)** — Decodes the RAW at full resolution to check the actual detail (loading may take a moment). Used manually only when needed
- **EXIF overlay (I)** · **RGB histogram (H)**
- **Photo background color** — change the viewer background in settings (presets: black/gray/white, incl. Lightroom's 50% gray / **custom HEX·RGB**)
- **Auto portrait/landscape rotation**, RAW+JPG pairing, RAW+ badge
- **Transfer** — Copy/**move** only the chosen files (by label · **rating** · **color tag**, folders per label/tag, companion files, serial numbers on conflict). On **Move**, the moved items are automatically cleared from the list. Shows a **progress bar and cancel** during transfer — never looks frozen on large folders or slow drives
- **Rename on transfer** — Copy or move while renaming by sequence number (1, 2, 3) / **rating grade (A1, B1…)** / a custom template (`{seq}` `{gradeseq}` `{stars}` `{tag}` `{orig}`, etc.) — with live preview, and identical names for RAW+JPG pairs
- **Auto-organize folder** — The **Organize** button (bottom of the left rail) sorts the photos in a folder into subfolders by **capture date / camera / lens / extension** (move or copy). EXIF-based criteria keep RAW+JPG pairs in the same folder; after organizing, the result opens right away for culling (separate from Transfer)
- **Update notice** — When idle after launch, checks GitHub for the latest release and, if a newer version exists, shows a notice button above the Organize button (click to open Releases)
- **Multilingual** — 한국어 / English / 日本語 (auto-detected from OS language, changeable and saved in settings)
- **Full screen** — Toggle OS-window full screen with **F11** or the toolbar button. Works from either single or grid view, and the flip / ORIG / zoom shortcuts all work in full screen too
- **Non-destructive saving** — Saves and restores sorting, ratings, and color tags in `.rawblow/session.json` (+ a human-readable txt) inside the folder
- **Fast loading** — Background decoding + forward preloading + LRU texture cache
- **Thumbnail disk cache** — Once a thumbnail is decoded, it is kept in the OS cache → even after you close and reopen the folder, it is **shown instantly without re-decoding**. Check and clear the size in settings + an **automatic cap (MB, user-configurable, default 1 GB)**; when exceeded, the oldest entries are cleaned up automatically

---

## Install / Run

### Download and run right away
Grab the file for your OS from [**Releases**](https://github.com/ascoeur9/rawblow/releases/latest).

- **Windows x64** — Run `RawBlow-Setup-vX.Y.Z.exe` to install (all required runtimes bundled). It is unsigned, so if SmartScreen shows a "Windows protected your PC" warning, click **More info → Run anyway**.
- **macOS (Apple Silicon)** — `RawBlow-vX.Y.Z-macos-arm64.zip` (unzip to get `RawBlow.app`)
  - On first launch, if you get an "unidentified developer" warning (not notarized): click **Done** in the dialog, then open **System Settings → Privacy & Security → Security** and click **Open Anyway** near the bottom. (Since macOS 15 Sequoia, the old **right-click → Open** bypass no longer works.)
  - Or, in a terminal, `xattr -dr com.apple.quarantine /Applications/RawBlow.app`

### Build from source (Linux, others)
```bash
cargo build --release -p rawblow-app   # 실행물: target/release/rawblow(.exe)
cargo run   --release -p rawblow-app
```
Requires: Rust 1.80+, a C linker (MSVC Build Tools / gcc / clang), and the Vulkan runtime on Linux. Korean fonts are loaded automatically from the OS fonts.

> 📌 For a **clean release build for distribution** (standalone + build path / username stripped), see [`BUILD.md`](BUILD.md).

---

## Usage

| Action | Key / Operation |
|------|-----------|
| Open folder | Top-left **Open folder** button · ⌘/Ctrl+O |
| Previous/next photo | ← / → |
| Up/down row in grid | ↑ / ↓ (auto-scrolls to follow the selection) |
| Pick / Hold / Reject / Clear | **Q** / **W** / **E** / **R** |
| Set / clear rating | **1 2 3 4 5** / **`** (backtick) — independent of labels |
| Set / clear color tag | **⇧1 ~ ⇧5** / **⇧0** — independent of labels and ratings |
| Single view ↔ grid | **T** |
| Fit to window ↔ 1:1 | **Click** the photo · Space · Z |
| Zoom in / out | **Ctrl+mouse wheel** · touchpad **pinch** |
| Pan while zoomed in | **Drag** the photo |
| Original view (ORIG) | **D** · toolbar **ORIG** |
| EXIF / histogram | **I** / **H** |
| Full screen | **F11** · toolbar **Full** (exit with ESC/F11) |
| Jump (go to number) | **G** |
| Switch filter | **F** |
| Transfer (copy/move selected files) | **Enter** · ⌘/Ctrl+E |
| Multi-select in grid | **Ctrl/⌘+click** (toggle) · click then **Shift+click** (range) |

---

## Tested environments

- **OS**: Windows 11 · macOS (Apple Silicon) officially released. Linux is supported in code but no prebuilt binary is provided (build from source).
- Other bodies and formats (other RAWs, JPG/PNG/HEIC, etc.) are made to work too, but **unverified bodies are displayed via the embedded preview path**. If you see gray/broken images or errors on a new body, please be sure to let us know.

### Tested camera RAWs (verified with real files)

#### Panasonic — `.RW2`
- LUMIX S1R II (`DC-S1RM2`)
- LUMIX S1 II (`DC-S1M2`)

#### Nikon — `.NEF`
- Z6III
- Z8
- Z30
- Z50II

#### Sony — `.ARW`
- α7R III
- α7C II

#### Fujifilm — `.RAF`
- GFX100S
- GFX100RF
- X-T5

#### Canon — `.CR2`
- EOS 5D
- EOS 5D Mark II

---

## Reporting issues / Feedback

For bugs (especially crashes), display errors, requests for new camera support, etc., use whichever is more convenient:

- **GitHub Issues** — https://github.com/ascoeur9/rawblow/issues
- **Email** — **hare.rinko@gmail.com**

**If a crash (forced termination) occurs, a `rawblow_crash.log` file is automatically created on the desktop.** Please attach it (or paste its contents). It helps a lot if you also include the following:

- What you were doing (e.g., holding down ↓ in the grid)
- Camera body / file format, roughly how many photos
- OS / GPU (if possible)

---

## Known limitations

- Color management goes from the embedded ICC → sRGB only (monitor ICC profile lookup is not applied; sRGB is assumed).
- No UI for rebinding shortcuts (defaults are fixed).
- Some formats such as HEIC rely on the platform decoder.
- ORIG original view displays within the GPU texture limit (about 8192px).

---

## Support

→ **[Donate via Toonation](https://toon.at/donate/hare)**

---

## Thanks To

Thanks to everyone who helped refine RawBlow through issue reports and testing:

- **Party!!** — Reported the issue where Canon EOS 5D Mark II (`.CR2`) was not displayed in single view (fixed in v0.2.7)
- **jebber** ([@dcjebber](https://x.com/dcjebber)) — Tested Sony α7C II (`.ARW`)
- **@stellar_sound** ([X](https://x.com/stellar_sound)) — Tested Nikon Z8 / Z30 / Z50II (`.NEF`), verified image loading on macOS
- **doer** — Tested Fujifilm GFX100S / GFX100RF (`.RAF`)
- **Laflat** — Tested Nikon Z6III (`.NEF`)
- **Agnes Digital** — Tested Fujifilm X-T5 (`.RAF`)

---

## License

Copyright © 2026 Hare. **All rights reserved.**

The source in this repository is published for **evaluation, testing, and feedback purposes only**. It may not be used, reproduced, modified, or distributed without the prior written permission of the copyright holder. See [`LICENSE`](LICENSE) for details. (Third-party libraries used follow their respective licenses.)

Contact: hare.rinko@gmail.com
