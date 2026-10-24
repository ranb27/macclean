- `screenshot-dark.png` / `screenshot-light.png` — theme-paired shots used via
  `<picture>` + `prefers-color-scheme` in the root README and landing page
  (copies in `docs/`). Retake with ⌘⇧4 → Space → click the app window.
- `icon-1024.png` — source icon (also embedded in the app UI + window icon);
  regenerate with `scripts/make_icon.py`.
- `macclean.icns` — bundle icon; rebuild with `sips` + `iconutil` after
  regenerating the PNG (see HANDOFF).
- `fonts/` — embedded Inter faces (SIL OFL, `LICENSE.txt` alongside).
