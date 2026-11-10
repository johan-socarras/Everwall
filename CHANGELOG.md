# Changelog

What changes in each version of Everwall. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the versions
follow [Semantic Versioning](https://semver.org/).

## [1.0.1] - Unreleased

### Added

- An About page in the settings window: the version, the copyright and
  license notice, where to find the source code, and the full texts of the
  GPL and of the third-party licenses. Both texts are built into the program,
  so a copy installed by copying only the program carries them too.

### Fixed

- Ubuntu: after a restart, every screen but the main one could stay grey until
  GNOME redrew the desktop. Everwall now waits for the GNOME session to settle
  before its first change, and keeps the wallpaper already set when it is its
  own and the screens have not moved.

## [1.0.0] - 2026-11-10

The first release, for Windows 11 and Ubuntu (GNOME).

### Added

- A different wallpaper on each screen, from a collection of folders per
  screen, or a different collection for morning, afternoon and night.
- Placement by shape: full screen when the image fits, a triptych of three
  images when it has the other shape, or the whole image over a blurred copy
  of itself. Every wallpaper is rendered at its screen's exact resolution and
  kept clear of the taskbar.
- Weighted random picks: favourites come up three times as often, and recent
  images or images already on another screen are skipped.
- Accent colour taken from the wallpaper, changed at most every few hours.
- A pause while a game or video is full screen, keyboard shortcuts, hidden
  images, and a button that gives back the wallpapers and accent colour from
  before Everwall.
- A tray icon and a settings window in English or Spanish.

[1.0.1]: https://github.com/johan-socarras/Everwall/compare/v1.0.0...v1.0.1
[1.0.0]: https://github.com/johan-socarras/Everwall/releases/tag/v1.0.0
