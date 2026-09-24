# Changelog

Every release of quvyta-browser, newest first. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## 0.1.1 - 2026-09-24

### Added

- Bookmarks: the star and ctrl+d keep a page, a bar below the address bar opens them (a middle click in a new tab, a right click to remove one), and the address bar suggests them while you type.

### Fixed

- A tab no longer stretches across the whole strip: tabs share it up to a desktop browser's width and scroll sideways once they no longer fit.

## 0.1.0 - 2026-09-24

The first version.

### Added

- A web browser inside the terminal: an unmodified Chromium runs headless, and its pages are drawn in real pixels where the terminal speaks kitty or sixel, with half blocks everywhere else.
- Tabs with a close mark and `+`, pages that open new windows opening them as tabs, back, forward, reload and stop, an address bar that also searches, and a loading mark.
- Clicks, the wheel, typing and pasting reach the page.
- A persistent profile of its own, a temporary one for a second window, and no Chromium process left behind.
- When Chromium is missing, an offer to install it in the terminal, or to show the command.
- Nine languages, and a notice when a newer version is out.
