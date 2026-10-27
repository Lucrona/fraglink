# Fraglink brand: "Split Orbit" (concept 28)

The Fraglink logo: an italic, blade-cut **F** with an orbit ring split around it.
Original Fraglink art (no third-party artwork).

| File | Use |
|---|---|
| `v28-split-orbit-mark.svg` | Flat white mark (small sizes, tray, 16/32 px icons) |
| `v28-split-orbit-mark-chrome.svg` | Chrome/silver mark (large icons, installer) |
| `v28-split-orbit-lockup.svg` | Flat white mark + wordmark (client header on the dark theme) |
| `v28-split-orbit-lockup-chrome.svg` | Chrome mark + wordmark (banner, README) |
| `v28-split-orbit-16.png`, `-32.png` | Hand-checked small renders |
| `v28-split-orbit-lockup-1600.png` | Large lockup render |
| `gen_icons.py` | Regenerates `apps/desktop/src-tauri/icons/*` (all Tauri sizes + `icon.ico`) and the NSIS `installer/*.bmp` from these SVGs (needs `rsvg-convert` and Pillow) |

All art is white/silver on a transparent background, designed for dark backgrounds.
The app icon (from dev.6) puts a charcoal mark on a light silver tile (the tray icon is unchanged).

## Font licence

The wordmark is set in **Saira Expanded Bold** and is converted to outlines in the
SVGs, so no font file ships with Fraglink. Saira is licensed under the
**SIL Open Font License, Version 1.1** (see `Saira-OFL.txt`). The OFL allows
using the font in logos and artwork; the outlined wordmark is artwork, not a
font, so it is not itself covered by the OFL.
- `gen_icons.py` (app icons, silver tile) and `gen_installer.py` (one-click installer splash `splash-*.bmp` and the uninstaller header BMP) regenerate the artwork.
