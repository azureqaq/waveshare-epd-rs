use std::{thread::sleep, time::Duration};

use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{Line, PrimitiveStyle, StyledDrawable},
};
use waveshare_epd::epd5in79::Epd5in79Impl;

fn main() {
    let mut epd_impl = Epd5in79Impl::default();
    {
        // binary color EPD.
        let mut epd_bin = epd_impl.as_binary();
        let style = PrimitiveStyle::with_stroke(BinaryColor::Off, 3);

        epd_bin
            .bounding_box()
            .draw_styled(&style, &mut epd_bin)
            .unwrap();

        Line::new(Point::new(792 / 2 - 8, 0), Point::new(792 / 2 + 8, 271))
            .draw_styled(&style, &mut epd_bin)
            .unwrap();

        epd_bin.display_binary_full().unwrap();
    }

    sleep(Duration::from_secs(3));

    {
        // gray2 color EPD.
        let mut epd_gray = epd_impl.as_gray2();
        epd_gray.display_gray2().unwrap();
        epd_gray.deep_sleep().unwrap();
    }
}
