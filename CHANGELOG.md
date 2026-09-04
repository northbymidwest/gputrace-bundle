# Changelog

Notable changes per release. Dates are the publish date.

## 0.1.1 - 2026-09-03

### Added

- `Bundle::record_count()`: the index header's total record count, an upper
  bound on the highest streamRef a fetch sweep needs to try (streamRefs are
  shared across all resource types, so this sits above `texture_count()`).

## 0.1.0 - 2026-09-03

### Added

- Initial release: a session-free reader for a `.gputrace` bundle's texture
  manifest (xdic index + zlib store).
