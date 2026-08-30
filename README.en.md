# RawBlow

[한국어](README.md) · **English** · [日本語](README.ja.md)

[![Windows: Microsoft Store](https://img.shields.io/badge/Windows-Microsoft%20Store-0078D4)](https://apps.microsoft.com/store/detail/9PC2FKGPQPD1)
[![Downloads](https://img.shields.io/github/downloads/ascoeur9/rawblow/total?label=downloads&color=brightgreen)](https://github.com/ascoeur9/rawblow/releases)
[![Latest release](https://img.shields.io/github/v/release/ascoeur9/rawblow?sort=semver&label=latest)](https://github.com/ascoeur9/rawblow/releases/latest)

**A culling viewer for flipping through RAW photos fast and picking the keepers.** Native Rust + egui/wgpu · Windows / macOS / Linux

A lightweight culling tool for flipping through thousands of RAWs, sorting them into Pick, Hold, and Reject, and then copying or moving only the ones you chose. It normally displays the preview embedded inside the RAW so flipping stays fast, and loads full resolution only when you need to inspect focus and detail. Your sorting goes into a sidecar file inside the folder and never touches the originals.

The paragraph above was written by an AI. The truth is that RawBlow exists because culling RAWs in Lightroom got so unbearably slow that I lost my temper, swung a fist at my desk, missed, and hit myself somewhere considerably more painful. This absurdly fast RAW viewer / culling tool is what came out of that...

Features keep getting added to make culling easier and faster, but "fast culling" comes before all of them.

---

## The basic loop

1. Open a folder. Use the **Open Folder** button at the top left, `⌘/Ctrl+O`, or drag a folder onto the window.
2. Flip with `←` `→` and press `Q` (Pick), `W` (Hold), or `E` (Reject). Star ratings `1`–`5` and color tags `⇧1`–`⇧5` are separate axes, so you can apply them at the same time.
3. Narrow the view down in the left rail, then press `Enter` to open Transfer and copy or move just the files you picked.

Your original files stay where they are the whole time. Labels, ratings, and tags accumulate in `.rawblow/session.json` inside the folder, and reopening the folder restores them.

---

## Features

### Viewing and flipping

- **Single view and grid.** Toggle with `T` between a single view with a film strip along the bottom and a thumbnail grid. Grid columns are configurable from 4 to 12.
- **Zoom.** Click a photo to switch between fit-to-window and 1:1. Zoom continuously with `Ctrl`+wheel or a touchpad pinch, and drag to pan while zoomed in. The magnification appears at the bottom right, where 100% means one original pixel per screen pixel. Zoom is carried over as you move between photos.
- **Original view (ORIG).** Decodes the RAW at full resolution instead of the embedded preview so you can check real detail. Turn it on with `D` or the **ORIG** button in the toolbar; depending on the file it may take a moment to load. Whether original view stays on as you flip is a setting.
- **Overlays.** `I` for EXIF, `H` for an RGB histogram, `A` for the AF points the camera locked onto, and `M` for a mini map of where a geotagged photo was taken.
- **Photo background color.** Change the viewer background with a preset (black, three grays, white) or by entering HEX/RGB directly. Lightroom's Develop default of 50% gray is among the presets.
- **Full screen.** `F11` or the **Full** button in the toolbar. Flipping, zoom, and original view shortcuts all keep working in full screen, from either single view or the grid.
- Automatic portrait/landscape rotation, RAW+JPG pairing, and a RAW+ badge.

### Sorting

- **Labels.** `Q` Pick, `W` Hold, `E` Reject, `R` Clear. Pressing the same key again clears it. Labeling advances to the next photo automatically, which you can turn off in settings.
- **Star ratings.** Set with `1`–`5` and clear with `` ` `` (backtick). Ratings are their own axis, so they apply alongside labels.
- **Color tags.** `⇧1`–`⇧5` apply orange, pink, teal, blue, and purple; `⇧0` removes the tag. Tags are independent of labels and ratings. Name each color in settings (by edit style, for example) and that name follows through into filters, transfers, and file names.
- **Multi-select in the grid.** `Ctrl`(`⌘`)+click toggles one at a time, and clicking then `Shift`+clicking selects a range. Sort the whole selection at once with `Q` `W` `E` `R` or a rating.
- **Batch relabel.** Press `B` in the grid. Paste file names or parts of them separated by newlines, commas, or tabs, and the matching items all get the label.
- **Undo.** `⌘/Ctrl+Z` undoes, `⌘/Ctrl+⇧Z` or `⌘/Ctrl+Y` redoes.
- **Non-destructive saving.** Labels, ratings, and color tags are saved to `.rawblow/session.json` inside the folder, along with a human-readable txt. The original files are left alone.

### Narrowing down

- **Filters.** Pick a label, a rating (exactly N stars), and a color tag in the left rail. The three are independent and stack with AND. Press `F` to cycle the label filter alone.
- **Jump.** Press `G` and enter a position or part of a file name to go straight there.
- **Resume per folder.** Each folder remembers the last photo you were on and starts there when you reopen it. It remembers the file path rather than the position in the list, so adding or deleting files or changing the sort order still lands on the same photo.
- **Sort order.** Capture time by default, switchable to file name in settings. When several cameras are mixed together, file names no longer match the shooting order.

### AI culling (experimental)

> **The model-driven checks are still being tested.** Face detection, AI sharpness, and object presence can get it wrong, so don't take the results at face value. No original file is ever deleted, and a culled shot only has its label, rating, or tag changed.

Analyzes your photos and marks them Good or Culled automatically. The result goes into just one axis you choose (label, rating, or color tag), so whatever you sorted by hand is left untouched. Everything runs locally on your machine, in the background, and can be canceled partway through.

- **Checks that need no model.** Focus (sharpness), exposure, and horizon tilt. Focus can be measured only within the AF points the camera locked onto instead of across the whole frame.
- **Metadata filters.** Portrait/landscape, ISO ceiling, focal length range, maximum aperture, minimum shutter speed, and partial matches on camera and lens names.
- **Burst best-N.** Groups shots taken within a chosen interval and keeps only the top N by score in each group.
- **Dedup.** Groups near-identical scenes with a perceptual hash and keeps only the best of each cluster.
- **Checks that need a model.** Aesthetic score (CLIP-IQA), AI sharpness, genre pick (portrait/landscape), face detection (YuNet), and object detection (YOLOv10n). Models download automatically on first use and are verified with sha256.
- **GPU acceleration.** WebGPU on Windows, CoreML on macOS. If registration fails it quietly falls back to the CPU.

### Getting files out

- **Transfer.** Copies or moves only the files you picked. Choose targets by label, rating, and color tag; anything matching at least one of the three is included (union). You can split the output into subfolders by label or tag, decide how companion files are handled, and set serial numbering for name conflicts. On Move, the moved items drop out of the list automatically. A progress bar and a cancel button mean it never looks frozen on large folders or slow drives.
- **Rename on transfer.** Choose sequence numbers (1, 2, 3), rating grades (A1, B1 …), or a custom template. Templates take `{seq}` `{gradeseq}` `{grade}` `{stars}` `{label}` `{tag}` `{orig}`, with zero padding as in `{seq:03}`. A live preview updates as you type, and RAW+JPG pairs get the same name.
- **Organize folder.** The **Organize** button at the bottom of the left rail sorts the photos in a folder into subfolders by capture date, camera, lens, focal length, or extension (move or copy). EXIF-based criteria keep RAW+JPG pairs in the same folder, and files whose EXIF can't be read collect in folders like `unknown-date`. You can open the organized folder and start culling right away; this is separate from Transfer.

### Everything else

- **Fast loading.** Embedded previews are decoded down to the size the screen needs, on top of background decoding, forward preloading, and an LRU texture cache.
- **Thumbnail disk cache.** Once decoded, thumbnails stay in the OS cache, so closing and reopening a folder shows them instantly with no re-decoding. Check and clear the cache in settings; when it exceeds the limit (1 GB by default, 0 for unlimited) the oldest entries are removed first.
- **Languages.** 한국어 / English / 日本語. Follows the OS language, and your choice in settings is saved.
- **Shortcut cheat sheet.** Press `?` or `F1` to see every shortcut inside the app.
- **Update notice.** When idle after launch, it checks GitHub for the latest release and shows a notice button in the left rail if a newer version exists. Can be turned off in settings.
- **Open source license notice.** Settings lists the bundled components and their full license texts.

---

## Keyboard shortcuts

Press them right over the photo. There is no text field, so they respond instantly.

| Action | Key / Operation |
|------|-----------|
| Open folder | **Open Folder** button at top left · `⌘/Ctrl+O` · drag a folder onto the window |
| Previous / next photo | `←` `→` · mouse wheel in single view when fit to window |
| Move a row in the grid | `↑` `↓` (auto-scrolls to follow the selection) |
| Jump | `G` |
| Batch relabel (grid) | `B` |
| Pick / Hold / Reject / Clear | `Q` `W` `E` `R` |
| Set / clear rating | `1` `2` `3` `4` `5` / `` ` `` |
| Set / clear color tag | `⇧1` – `⇧5` / `⇧0` |
| Undo / redo | `⌘/Ctrl+Z` / `⌘/Ctrl+⇧Z` · `⌘/Ctrl+Y` |
| Single view ↔ grid | `T` |
| Fit to window ↔ 1:1 | click the photo · `Space` · `Z` |
| Zoom in / out | `Ctrl`+wheel · touchpad pinch |
| Pan while zoomed in | drag the photo |
| Original view (ORIG) | `D` · toolbar **ORIG** |
| EXIF / histogram | `I` / `H` |
| AF points / location map | `A` / `M` |
| Cycle label filter | `F` |
| Full screen | `F11` · toolbar **Full** (exit with `Esc`) |
| Multi-select in the grid | `Ctrl/⌘`+click (toggle) · click then `Shift`+click (range) |
| Transfer | `Enter` · `⌘/Ctrl+E` |
| Shortcut help | `?` · `F1` |

---

## Install

### Windows (Microsoft Store recommended)

[**Get it on the Microsoft Store**](https://apps.microsoft.com/store/detail/9PC2FKGPQPD1). Installing from the Store keeps it updated automatically and launches without security warnings.

Only if you can't use the Microsoft Store, download `RawBlow-Setup-vX.Y.Z.exe` from [**Releases**](https://github.com/ascoeur9/rawblow/releases/latest). Every runtime it needs is bundled, so there is nothing else to install. It is unsigned, so if SmartScreen shows "Windows protected your PC" on first launch, click **More info → Run anyway**.

### macOS (Apple Silicon)

Download `RawBlow-vX.Y.Z-macos-arm64.zip` from [**Releases**](https://github.com/ascoeur9/rawblow/releases/latest) and unzip it to get `RawBlow.app`.

It isn't notarized, so the first launch shows an "unidentified developer" warning. Click **Done** in the dialog, then open **System Settings → Privacy & Security → Security** and click **Open Anyway** near the bottom. Since macOS 15 Sequoia, the old right-click → Open workaround no longer works. From a terminal, this one line does the same thing.

```bash
xattr -dr com.apple.quarantine /Applications/RawBlow.app
```

### Build from source (Linux and others)

```bash
cargo build --release -p rawblow-app   # binary: target/release/rawblow(.exe)
cargo run   --release -p rawblow-app
```

You need Rust 1.80 or later and a C linker (MSVC Build Tools / gcc / clang); Linux also needs the Vulkan runtime. Korean fonts are loaded from the OS fonts automatically.

For a clean release build for distribution (standalone, with build paths and user names stripped), see [`BUILD.md`](BUILD.md).

---

## Supported formats and verified cameras

RawBlow reads `.ARW` `.CR2` `.CR3` `.NEF` `.ORF` `.RW2` `.RAF` `.DNG` `.PEF` `.SRW` `.RAW` for RAW, and `.JPG` `.JPEG` `.PNG` `.WEBP` `.HEIC` `.HEIF` `.TIF` `.TIFF` for regular images.

**OS.** Officially released for Windows 11 and macOS (Apple Silicon). Linux is supported in code but ships no prebuilt binary, so build from source.

Bodies outside the list below are meant to work too, but any body that hasn't been verified is displayed through the embedded preview path. If you see a gray screen, broken images, or errors on a new body, please tell us.

### Cameras verified with real files

| Maker | Format | Bodies |
|---|---|---|
| Panasonic | `.RW2` | LUMIX S1R II (`DC-S1RM2`), LUMIX S1 II (`DC-S1M2`) |
| Nikon | `.NEF` | Z6III, Z8, Z30, Z50II, D850 |
| Sony | `.ARW` | α7R III, α7C II |
| Fujifilm | `.RAF` | GFX100S, GFX100RF, X-T5 |
| Canon | `.CR2` / `.CR3` | EOS 5D, EOS 5D Mark II / EOS R6 Mark III |
| OM SYSTEM · Olympus | `.ORF` | OM-1, E-300 |
| Pentax | `.PEF` | K-1 |

---

## Reporting issues

For bugs (especially crashes), display errors, or requests to support a new camera, use whichever is easier.

- **GitHub Issues** [github.com/ascoeur9/rawblow/issues](https://github.com/ascoeur9/rawblow/issues)
- **Email** hare.rinko@gmail.com

If the app crashes, a `rawblow_crash.log` file is created on your desktop automatically. Please attach it or paste its contents. These details make the cause much easier to find.

- What you were doing (holding `↓` down in the grid, for example)
- Camera body and file format, and roughly how many photos
- OS and GPU, if you can

---

## Known limitations

- Color management goes from the embedded ICC profile to sRGB. The monitor's ICC profile is not consulted; sRGB is assumed.
- There is no screen for rebinding shortcuts. The defaults are fixed.
- Some formats such as HEIC rely on the platform decoder.
- Original view displays within the GPU texture limit (roughly 8192 px on the long edge).
- Pentax files record no lens name anywhere, so the lens field stays empty.
- The model-driven checks in AI culling are still experimental.

---

## Support

→ [**Donate via Toonation**](https://toon.at/donate/hare)

---

## Thanks

Thanks to everyone who has helped refine RawBlow with issue reports and testing.

- **Party!!** Reported that Canon EOS 5D Mark II (`.CR2`) would not display in single view (fixed in v0.2.7)
- **jebber** ([@dcjebber](https://x.com/dcjebber)) Tested Sony α7C II (`.ARW`)
- **@stellar_sound** ([X](https://x.com/stellar_sound)) Tested Nikon Z8 / Z30 / Z50II (`.NEF`), verified image loading on macOS
- **doer** Tested Fujifilm GFX100S / GFX100RF (`.RAF`)
- **Laflat** Tested Nikon Z6III (`.NEF`)
- **Agnes Digital** Tested Fujifilm X-T5 (`.RAF`)

---

## License

Copyright © 2026 Hare. **All rights reserved.**

The source in this repository is published for evaluation, testing, and feedback only. It may not be used, reproduced, modified, or distributed without prior written permission from the copyright holder. See [`LICENSE`](LICENSE) for details. Third-party libraries follow their own licenses.

Contact: hare.rinko@gmail.com
