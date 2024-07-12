# waveshare-epd-rs
Unofficial rust implementation of the waveshare e-paper driver.

Currently, only `epd5in79` is supported. We welcome issues and pull requests!

# How to use
## Add dependencies
```toml
# Cargo.toml
[dependencies]
waveshare_epd = { git = "https://github.com/azureqaq/waveshare-epd-rs.git", features = [
    "epd5in79",
] }
```
## Import crate and use it
```rust
use waveshare_epd::epd5in79::Epd5in79Impl;
use embedded_graphics::draw_target::DrawTarget;

fn main() {
    let mut epd_impl = Epd5in79Impl::default();
    // As `BinaryColor` driver.
    let mut epd_bin = epd_impl.as_binary();
    // Use `embedded_graphics` to draw some pixels...
    epd_bin.display_binary_fast().unwrap();
    drop(epd_bin);
    
    // As `Gray2` driver.
    let mut epd_gray = epd_impl.as_gray2();
    // Draw some pixels...
    epd_gray.display_gray2().unwrap();

    // When `epd_impl` goes out of scope, it will automatically enter deep sleep mode.
}
```