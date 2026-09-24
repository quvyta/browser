# Changelog

Every release of quvyta-browser, newest first. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## Unreleased

The first version.

### Added

- A web browser inside the terminal: an unmodified Chromium runs headless, and its pages are drawn in real pixels where the terminal speaks kitty or sixel, with half blocks everywhere else.
- Tabs with a close mark and `+`, pages that open new windows opening them as tabs, back, forward, reload and stop, an address bar that also searches, and a loading mark.
- Clicks, the wheel, typing and pasting reach the page.
- A persistent profile of its own, a temporary one for a second window, and no Chromium process left behind.
- When Chromium is missing, an offer to install it in the terminal, or to show the command.
- Nine languages, and a notice when a newer version is out.
