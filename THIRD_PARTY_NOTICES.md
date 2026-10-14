# Third-party notices

Project code is MIT. Third-party crates and models keep their own licenses.

Rust dependencies (see `Cargo.lock`) are used under their upstream licenses, mostly MIT or Apache-2.0. They are fetched at build time and are not vendored in this repository.

Device connectivity, e-ink rasterization, and the shared speech environment belong to ZECTRIX Hub. This app does not link the Hub SDK, FreeType, or libpng. Chinese and Latin glyphs on the device are the fonts Hub supplies.

Kokoro-82M is installed separately through Hub. It is not part of this package and is not covered by this MIT license. Default voices are `af_heart` (English) and `zf_xiaobei` (Mandarin). See the [Kokoro model card](https://huggingface.co/hexgrad/Kokoro-82M).

News text is fetched at runtime from the feeds in `sources.toml`. Those articles remain the property of their publishers. The 机器之心 entry is a public WeChat-channel mirror, not an official 机器之心 feed; swap the URL if you host your own.

No API keys, model weights, device addresses, or personal reading history are stored in this repository.
