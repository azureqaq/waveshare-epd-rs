use std::{thread::sleep, time::Duration};

use embedded_graphics::{
    pixelcolor::{BinaryColor, Gray2},
    prelude::*,
    primitives::{Circle, Line, PrimitiveStyle, Rectangle, StyledDrawable},
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

        epd_bin.deep_sleep().unwrap();

        epd_bin.display_binary_full().unwrap();

        sleep(Duration::from_secs(3));

        Circle::with_center(epd_bin.bounding_box().center(), 100)
            .draw_styled(&style, &mut epd_bin)
            .unwrap();
        epd_bin.display_binary_fast().unwrap();

        sleep(Duration::from_secs(3));

        for i in 6..10 {
            Circle::with_center(epd_bin.bounding_box().center(), i * 20)
                .draw_styled(&style, &mut epd_bin)
                .unwrap();
            epd_bin.display_binary_partial().unwrap();
            sleep(Duration::from_secs(1));
        }
    }

    sleep(Duration::from_secs(2));

    {
        // gray2 color EPD.
        let mut epd_gray = epd_impl.as_gray2();
        Rectangle::new((0, 0).into(), (100, 50).into())
            .draw_styled(&PrimitiveStyle::with_fill(Gray2::BLACK), &mut epd_gray)
            .unwrap();
        Rectangle::new((0, 50).into(), (100, 50).into())
            .draw_styled(&PrimitiveStyle::with_fill(Gray2::new(1)), &mut epd_gray)
            .unwrap();
        Rectangle::new((0, 100).into(), (100, 50).into())
            .draw_styled(&PrimitiveStyle::with_fill(Gray2::new(2)), &mut epd_gray)
            .unwrap();
        epd_gray.display_gray2().unwrap();
    }
}
