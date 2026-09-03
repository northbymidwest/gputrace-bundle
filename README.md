# gputrace-bundle

A session-free reader for a `.gputrace` capture bundle's texture manifest.

`.gputrace` is the bundle Apple's GPU tools write when you capture a Metal
frame. This crate reads the on-disk texture index directly, no replay session
and no private framework required: it parses the `xdic` index and the zlib
store to enumerate a capture's textures and their raw descriptor fields
(dimensions, format, mip/array counts, and so on).

It is pure, dependency-light Rust with **no `unsafe`**, no `objc2`, and no tie
to macOS: the parsing runs anywhere, though a `.gputrace` is only produced by
Apple's tooling. For live replay, fetch, and playback of a capture, see the
[`gputools-replay`](https://github.com/northbymidwest/gputools-replay) stack.

## Status

Pre-release (`0.x`). The descriptor fields are reverse-engineered from observed
captures and may gain coverage over time.

## License

`0BSD`. See [LICENSE](LICENSE).
