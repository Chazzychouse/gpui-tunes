//! Turn embedded picture bytes into a gpui image.

use std::sync::Arc;

use gpui_kit::{Image, ImageFormat};

/// Wrap encoded picture bytes for `img()`. Format is sniffed from magic bytes.
#[allow(dead_code)] // consumed by the now-playing artwork view in a later task
pub fn decode(bytes: &[u8]) -> Arc<Image> {
    let format = match bytes {
        [0x89, b'P', b'N', b'G', ..] => ImageFormat::Png,
        [
            b'R',
            b'I',
            b'F',
            b'F',
            _,
            _,
            _,
            _,
            b'W',
            b'E',
            b'B',
            b'P',
            ..,
        ] => ImageFormat::Webp,
        [b'G', b'I', b'F', ..] => ImageFormat::Gif,
        [b'B', b'M', ..] => ImageFormat::Bmp,
        _ => ImageFormat::Jpeg,
    };
    Arc::new(Image::from_bytes(format, bytes.to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_png_and_defaults_to_jpeg() {
        assert!(matches!(decode(b"\x89PNG\r\n").format(), ImageFormat::Png));
        assert!(matches!(
            decode(b"\xFF\xD8\xFF").format(),
            ImageFormat::Jpeg
        ));
    }
}
