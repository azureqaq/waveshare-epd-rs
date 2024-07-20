//! Implement the driver for **epd5in79**.
//!
//! This screen supports two colors ([`BinaryColor`], [`Gray2`]).
//! This driver supports automatic color conversion.
//!
//! # Examples
//! ```no_run
//! # use waveshare_epd::epd5in79::Epd5in79Impl;
//! let mut epd_impl = Epd5in79Impl::default();
//! let mut epd_gray = epd_impl.as_gray2();
//! // Draw some pixels...
//! epd_gray.display_gray2().unwrap();
//!
//! // When you need to switch to another color's driver,
//! // you must explicitly drop the previous mutable reference.
//! // Alternatively, you can create a scope(`{}`) to have it automatically dropped.
//! drop(epd_gray);
//!
//! // When switching drivers, the buffer will automatically undergo color mapping.
//! let mut epd_binary = epd_impl.as_binary();
//! // Draw some pixels...
//! epd_binary.display_binary_full().unwrap();
//! // Draw some pixels...
//! epd_binary.display_binary_fast().unwrap();
//!
//! // When `epd_impl` goes out of scope, it will automatically enter deep sleep mode,
//! // at this point, any errors will be ignored,
//! // and you can explicitly call `deepsleep()` to enter deep sleep mode.
//! ```

use std::{
    convert::Infallible,
    fmt::Debug,
    marker::PhantomData,
    path::Path,
    time::{Duration, Instant},
};

use embedded_graphics_core::{
    image::GetPixel,
    pixelcolor::{BinaryColor, Gray2},
    prelude::*,
};
use linux_embedded_hal::{
    gpio_cdev::{Chip, LineRequestFlags},
    spidev::{SpiModeFlags, SpidevOptions},
    CdevPin, Delay, SpidevDevice,
};
use waveshare_epd_core::spi_interface::{DelayStep, PinDefinition, SpiInterface};

// TODO: use specialised error types.
type Spi = SpiInterface<SpidevDevice, CdevPin, CdevPin, Delay, anyhow::Error>;

pub const WIDTH: u32 = 792;
pub const HIGH: u32 = 272;

pub struct Epd5in79Impl {
    spi_interface: Spi,
    buffer0: Box<[u8; 13600]>, // master bw 0x24
    buffer1: Box<[u8; 13600]>, // slave bw 0xa4
    buffer2: Box<[u8; 13600]>, // master r 0x26
    buffer3: Box<[u8; 13600]>, // slave r 0xa6
    state: Epd5in79State,
}

impl Debug for Epd5in79Impl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Epd5in79Impl")
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

impl Default for Epd5in79Impl {
    /// Use default [`PinDefinition`] and `/dev/spidev0.0` `/dev/gpiochip0`.
    fn default() -> Self {
        Self::new_with_pindefinition(PinDefinition::DEFAULT, "/dev/spidev0.0", "/dev/gpiochip0")
            .unwrap()
    }
}

impl Epd5in79Impl {
    pub fn new(
        spi: SpidevDevice,
        rst_pin: CdevPin,
        dc_pin: CdevPin,
        cs_pin: Option<CdevPin>,
        busy_pin: CdevPin,
        pwr_pin: CdevPin,
        delay: Delay,
    ) -> Self {
        let buf = Box::new([!0; 13600]);
        Self {
            spi_interface: SpiInterface::new(
                spi, rst_pin, dc_pin, cs_pin, busy_pin, pwr_pin, delay,
            ),
            buffer0: buf.clone(),
            buffer1: buf.clone(),
            buffer2: buf.clone(),
            buffer3: buf,
            state: Epd5in79State {
                power_on: None,
                color_in_buf: ColorInBuf::Binary,
                init_for: None,
            },
        }
    }

    pub fn new_with_pindefinition(
        pindefinition: PinDefinition,
        spi_path: impl AsRef<Path>,
        gpio_path: impl AsRef<Path>,
    ) -> Result<Self, anyhow::Error> {
        let mut spi = SpidevDevice::open(spi_path)?;
        spi.0.configure(
            &SpidevOptions::new()
                .max_speed_hz(4_000_000)
                .mode(SpiModeFlags::SPI_MODE_0)
                .build(),
        )?;
        let mut chip = Chip::new(gpio_path)?;
        let rst_pin = CdevPin::new(chip.get_line(pindefinition.rst_pin)?.request(
            LineRequestFlags::OUTPUT,
            0,
            "epd5in79_rst_pin",
        )?)?;
        let dc_pin = CdevPin::new(chip.get_line(pindefinition.dc_pin)?.request(
            LineRequestFlags::OUTPUT,
            0,
            "epd5in79_dc_pin",
        )?)?;
        let pwr_pin = CdevPin::new(chip.get_line(pindefinition.pwr_pin)?.request(
            LineRequestFlags::OUTPUT,
            0,
            "epd5in79_pwr_pin",
        )?)?;
        let busy_pin = CdevPin::new(chip.get_line(pindefinition.busy_pin)?.request(
            LineRequestFlags::INPUT | LineRequestFlags::from_bits_retain(1 << 6),
            0,
            "epd5in79_busy_pin",
        )?)?;
        let cs_pin = if let Some(cs_pin_n) = pindefinition.cs_pin {
            Some(CdevPin::new(chip.get_line(cs_pin_n)?.request(
                LineRequestFlags::OUTPUT | LineRequestFlags::ACTIVE_LOW,
                0,
                "epd5in79_cs_pin",
            )?)?)
        } else {
            None
        };
        Ok(Self::new(
            spi, rst_pin, dc_pin, cs_pin, busy_pin, pwr_pin, Delay,
        ))
    }

    pub fn as_binary(&mut self) -> Epd5in79<'_, BinaryColor> {
        self.as_binary_with(BinaryColor::from)
    }

    pub fn as_binary_with(
        &mut self,
        f: impl Fn(Gray2) -> BinaryColor,
    ) -> Epd5in79<'_, BinaryColor> {
        self.mapping_to_binary(f);
        Epd5in79 {
            inner: self,
            color: PhantomData,
        }
    }

    pub fn as_gray2(&mut self) -> Epd5in79<'_, Gray2> {
        self.as_gray2_with(Gray2::from)
    }

    pub fn as_gray2_with(&mut self, f: impl Fn(BinaryColor) -> Gray2) -> Epd5in79<'_, Gray2> {
        self.mapping_to_gray2(f);
        Epd5in79 {
            inner: self,
            color: PhantomData,
        }
    }

    fn mapping_to_binary(&mut self, f: impl Fn(Gray2) -> BinaryColor) {
        if matches!(self.state.color_in_buf, ColorInBuf::Binary) {
            return;
        }
        for x in 0..792 {
            for y in 0..272 {
                let point = Point::new(x, y);
                // get color
                let Some(color) = self.get_gray(point) else {
                    continue;
                };
                // set pixel
                self.set_binary(Pixel(point, f(color)));
            }
        }
        self.state.color_in_buf = ColorInBuf::Binary;
    }

    fn mapping_to_gray2(&mut self, f: impl Fn(BinaryColor) -> Gray2) {
        if matches!(self.state.color_in_buf, ColorInBuf::Gray) {
            return;
        }
        for x in 0..792 {
            for y in 0..272 {
                let point = Point::new(x, y);
                // get color
                let Some(color) = self.get_binary(point) else {
                    continue;
                };
                // set pixel
                self.set_gray(Pixel(point, f(color)));
            }
        }
        self.state.color_in_buf = ColorInBuf::Gray;
    }

    fn send_buf(&mut self, cmd: u8) -> Result<(), anyhow::Error> {
        let buf = match cmd {
            0x24 => self.buffer0.as_slice(),
            0xa4 => self.buffer1.as_slice(),
            0x26 => self.buffer2.as_slice(),
            0xa6 => self.buffer3.as_slice(),
            _ => unreachable!(),
        };
        self.spi_interface.command_data(cmd, buf, 4096)?;
        Ok(())
    }

    fn send_bufs(&mut self, cmds: impl IntoIterator<Item = u8>) -> Result<(), anyhow::Error> {
        for cmd in cmds {
            self.send_buf(cmd)?;
        }
        Ok(())
    }

    fn send_bufs_all(&mut self) -> Result<(), anyhow::Error> {
        self.send_bufs([0x24, 0x26, 0xa4, 0xa6])
    }

    fn command_data(&mut self, cmd: u8, data: impl AsRef<[u8]>) -> Result<(), anyhow::Error> {
        self.spi_interface.command_data(cmd, data, 4096)?;
        Ok(())
    }

    pub fn deep_sleep(&mut self) -> Result<(), anyhow::Error> {
        if !self.state.is_deepsleep() {
            self.spi_interface.command_data(0x10, [0x03], 4096)?;
            self.state.power_on = None;
            self.spi_interface.set_power(false)?;
            self.spi_interface.set_rst_pin(false)?;
        }
        Ok(())
    }

    pub fn power_on_dur(&self) -> Option<Duration> {
        self.state.power_on.map(|i| i.elapsed())
    }

    fn set_binary(&mut self, Pixel(point, color): Pixel<BinaryColor>) {
        if !is_point_in_screen(point) {
            return;
        }

        if point.x < 50 * 8 {
            // master
            let buf_index = 50 * point.y + point.x / 8;
            let offset = 7 - (point.x % 8) as u8;
            let value = self.buffer0.get_mut(buf_index as usize).unwrap();
            set_binary_value(color, offset, value);
        }

        if point.x >= 49 * 8 {
            // slave
            let buf_index = 50 * point.y + (point.x - 49 * 8) / 8;
            let offset = 7 - ((point.x - 49 * 8) % 8) as u8;
            let value = self.buffer1.get_mut(buf_index as usize).unwrap();
            set_binary_value(color, offset, value);
        }
    }

    fn get_binary(&self, Point { x, y }: Point) -> Option<BinaryColor> {
        if !is_point_in_screen(Point::new(x, y)) {
            return None;
        }
        if x < 49 * 8 {
            // master
            let buf_index = 50 * y + x / 8;
            let offset = 7 - (x % 8) as u8;
            let value = self.buffer0[buf_index as usize];
            Some(get_binary_from_value(offset, value))
        } else {
            // slave
            let buf_index = 50 * y + (x - 49 * 8) / 8;
            let offset = 7 - ((x - 49 * 8) % 8) as u8;
            let value = self.buffer1[buf_index as usize];
            Some(get_binary_from_value(offset, value))
        }
    }

    fn set_gray(&mut self, Pixel(point, color): Pixel<Gray2>) {
        if !is_point_in_screen(point) {
            return;
        }

        if point.x < 50 * 8 {
            // master
            let buf_index = 50 * point.y + point.x / 8;
            let offset = 7 - (point.x % 8) as u8;
            let bw_value = self.buffer0.get_mut(buf_index as usize).unwrap();
            let r_value = self.buffer2.get_mut(buf_index as usize).unwrap();
            set_gray_value(color, offset, bw_value, r_value);
        }

        if point.x >= 49 * 8 {
            // slave
            let buf_index = 50 * point.y + (point.x - 49 * 8) / 8;
            let offset = 7 - ((point.x - 49 * 8) % 8) as u8;
            let bw_value = self.buffer1.get_mut(buf_index as usize).unwrap();
            let r_value = self.buffer3.get_mut(buf_index as usize).unwrap();
            set_gray_value(color, offset, bw_value, r_value);
        }
    }

    fn get_gray(&self, Point { x, y }: Point) -> Option<Gray2> {
        if !is_point_in_screen(Point::new(x, y)) {
            return None;
        }

        if x < 49 * 8 {
            // master
            let buf_index = 50 * y + x / 8;
            let offset = 7 - (x % 8) as u8;
            let bw_value = self.buffer0[buf_index as usize];
            let r_value = self.buffer2[buf_index as usize];
            Some(get_gray_from_values(offset, bw_value, r_value))
        } else {
            // slave
            let buf_index = 50 * y + (x - 49 * 8) / 8;
            let offset = 7 - ((x - 49 * 8) % 8) as u8;
            let bw_value = self.buffer1[buf_index as usize];
            let r_value = self.buffer3[buf_index as usize];
            Some(get_gray_from_values(offset, bw_value, r_value))
        }
    }
}

impl Drop for Epd5in79Impl {
    fn drop(&mut self) {
        let _ = self.deep_sleep();
    }
}

#[derive(Debug)]
pub struct Epd5in79<'a, C> {
    inner: &'a mut Epd5in79Impl,
    color: PhantomData<C>,
}

impl<'a> GetPixel for Epd5in79<'a, BinaryColor> {
    type Color = BinaryColor;
    fn pixel(&self, p: Point) -> Option<Self::Color> {
        debug_assert!(matches!(self.inner.state.color_in_buf, ColorInBuf::Binary));
        self.inner.get_binary(p)
    }
}

impl<'a> GetPixel for Epd5in79<'a, Gray2> {
    type Color = Gray2;
    fn pixel(&self, p: Point) -> Option<Self::Color> {
        debug_assert!(matches!(self.inner.state.color_in_buf, ColorInBuf::Gray));
        self.inner.get_gray(p)
    }
}

impl<'a, C> std::ops::Deref for Epd5in79<'a, C> {
    type Target = Epd5in79Impl;

    fn deref(&self) -> &Self::Target {
        self.inner
    }
}

impl<'a, C> std::ops::DerefMut for Epd5in79<'a, C> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner
    }
}

impl<'a, C> Epd5in79<'a, C> {
    fn set_address(&mut self) -> Result<(), anyhow::Error> {
        self.inner.command_data(0x11, [0x01])?;

        self.inner.command_data(0x44, [0x00, 0x31])?;
        self.inner.command_data(0x45, [0x0f, 0x01, 0x00, 0x00])?;

        self.inner.command_data(0x4e, [0x00])?;
        self.inner.command_data(0x4f, [0x0f, 0x01])?;

        self.inner.command_data(0x91, [0x00])?;
        self.inner.command_data(0xc4, [0x31, 0x00])?;
        self.inner.command_data(0xc5, [0x0f, 0x01, 0x00, 0x00])?;

        self.inner.command_data(0xce, [0x31])?;
        self.inner.command_data(0xcf, [0x0f, 0x01])?;
        Ok(())
    }

    fn check_deepsleep(&self) -> Result<(), anyhow::Error> {
        self.inner.state.check_deepsleep()
    }

    fn hw_reset(&mut self) -> Result<(), anyhow::Error> {
        self.inner.spi_interface.set_rst_pin(true)?;
        self.inner.spi_interface.delay(DelayStep::Us(200));
        self.inner.spi_interface.set_rst_pin(false)?;
        self.inner.spi_interface.delay(DelayStep::Us(200));
        self.inner.spi_interface.set_rst_pin(true)?;
        self.inner.spi_interface.delay(DelayStep::Us(200));
        self.wait_busy_without_check()?;
        self.inner.state.power_on = Some(Instant::now());
        Ok(())
    }

    fn sw_reset(&mut self) -> Result<(), anyhow::Error> {
        self.inner.spi_interface.command(0x12)?;
        self.wait_busy_without_check()?;
        Ok(())
    }

    fn power_on(&mut self) -> Result<(), anyhow::Error> {
        if self.inner.state.is_deepsleep() {
            self.inner.spi_interface.set_power(true)?;
            self.hw_reset()?;
        }
        self.sw_reset()?;
        self.inner.state.init_for = None;
        Ok(())
    }

    fn wait_busy_without_check(&mut self) -> Result<(), anyhow::Error> {
        self.inner
            .spi_interface
            .wait_busy_timeout(DelayStep::Us(200), Duration::from_secs(5))?;
        Ok(())
    }

    pub fn wait_busy(&mut self) -> Result<(), anyhow::Error> {
        self.check_deepsleep()?;
        self.wait_busy_without_check()?;
        Ok(())
    }
}

impl<'a> Epd5in79<'a, Gray2> {
    fn init_gray2(&mut self) -> Result<(), anyhow::Error> {
        self.power_on()?;
        self.inner.command_data(0x0c, [0x8b, 0x9c, 0xa6, 0x0f])?;
        self.inner.command_data(0x3c, [0x81])?;
        self.set_address()?;
        self.load_lut()?;
        self.inner.state.init_for = Some(DisplayMode::Gray2);
        Ok(())
    }

    fn ensure_inited_gray2(&mut self) -> Result<(), anyhow::Error> {
        if !self.inner.state.is_ready_for(DisplayMode::Gray2) {
            self.init_gray2()?;
        }
        Ok(())
    }

    fn load_lut(&mut self) -> Result<(), anyhow::Error> {
        self.inner.command_data(0x32, LUT_DATA)?;
        self.inner.command_data(0x3f, [0x22])?;
        self.inner.command_data(0x03, [0x17])?;
        self.inner.command_data(0x04, [0x41, 0xa8, 0x32])?;
        self.inner.command_data(0x2c, [0x40])?;
        Ok(())
    }

    pub fn display_gray2(&mut self) -> Result<(), anyhow::Error> {
        self.ensure_inited_gray2()?;
        debug_assert!(matches!(self.inner.state.color_in_buf, ColorInBuf::Gray));

        // send data
        self.inner.send_bufs_all()?;
        // turn on display
        self.inner.command_data(0x22, [0xcf])?;
        self.inner.spi_interface.command(0x20)?;

        self.inner.spi_interface.delay(DelayStep::Us(200));
        self.wait_busy()?;
        Ok(())
    }
}

impl<'a> Epd5in79<'a, BinaryColor> {
    fn init_binary_full(&mut self) -> Result<(), anyhow::Error> {
        self.power_on()?;
        self.set_address()?;
        self.inner.state.init_for = Some(DisplayMode::Full);
        Ok(())
    }

    fn ensure_inited_binary_full(&mut self) -> Result<(), anyhow::Error> {
        if !self.inner.state.is_ready_for(DisplayMode::Full) {
            self.init_binary_full()?;
        }
        debug_assert!(self.inner.state.is_ready_for(DisplayMode::Full));
        Ok(())
    }

    pub fn display_binary_full(&mut self) -> Result<(), anyhow::Error> {
        self.ensure_inited_binary_full()?;

        // send data
        self.inner.buffer2.fill(0);
        self.inner.buffer3.fill(0);
        self.inner.send_bufs_all()?;
        // turn on display
        self.inner.command_data(0x22, [0xf7])?;
        self.inner.spi_interface.command(0x20)?;

        self.inner.spi_interface.delay(DelayStep::Us(200));
        self.wait_busy()?;
        Ok(())
    }

    fn init_binary_fast(&mut self) -> Result<(), anyhow::Error> {
        self.power_on()?;

        self.inner.command_data(0x18, [0x80])?;
        self.inner.command_data(0x22, [0xb1])?;
        self.inner.spi_interface.command(0x20)?;
        self.wait_busy()?;

        self.inner.command_data(0x1a, [0x64, 0x00])?;
        self.inner.command_data(0x22, [0x91])?;
        self.inner.spi_interface.command(0x20)?;
        self.wait_busy()?;

        self.set_address()?;

        self.inner.state.init_for = Some(DisplayMode::Fast);
        Ok(())
    }

    fn ensure_inited_binary_fast(&mut self) -> Result<(), anyhow::Error> {
        if !self.inner.state.is_ready_for(DisplayMode::Fast) {
            self.init_binary_fast()?;
        }
        debug_assert!(self.inner.state.is_ready_for(DisplayMode::Fast));
        Ok(())
    }

    pub fn display_binary_fast(&mut self) -> Result<(), anyhow::Error> {
        self.ensure_inited_binary_fast()?;

        // send data
        self.inner.buffer2.fill(0);
        self.inner.buffer3.fill(0);
        self.inner.send_bufs_all()?;
        // turn on display
        self.inner.command_data(0x22, [0xc7])?;
        self.inner.spi_interface.command(0x20)?;

        self.inner.spi_interface.delay(DelayStep::Us(200));
        self.wait_busy()?;
        Ok(())
    }

    fn init_binary_partial(&mut self) -> Result<(), anyhow::Error> {
        self.power_on()?;
        self.inner.command_data(0x3c, [0x80])?;
        self.set_address()?;
        self.inner
            .buffer2
            .copy_from_slice(self.inner.buffer0.as_slice());
        self.inner
            .buffer3
            .copy_from_slice(self.inner.buffer1.as_slice());
        self.inner.send_bufs([0x26, 0xa6])?;
        self.inner.state.init_for = Some(DisplayMode::Partial);
        Ok(())
    }

    fn ensure_inited_binary_partial(&mut self) -> Result<(), anyhow::Error> {
        if !self.inner.state.is_ready_for(DisplayMode::Partial) {
            self.init_binary_partial()?;
        }
        debug_assert!(self.inner.state.is_ready_for(DisplayMode::Partial));
        Ok(())
    }

    pub fn display_binary_partial(&mut self) -> Result<(), anyhow::Error> {
        self.ensure_inited_binary_partial()?;

        // send buffer
        self.inner.send_bufs([0x24, 0xa4])?;
        // turn on display
        self.inner.command_data(0x22, [0xff])?;
        self.inner.spi_interface.command(0x20)?;

        self.inner.spi_interface.delay(DelayStep::Us(200));
        self.wait_busy()?;
        Ok(())
    }
}

impl<'a, C> OriginDimensions for Epd5in79<'a, C> {
    fn size(&self) -> Size {
        (792, 272).into()
    }
}

impl<'a> DrawTarget for Epd5in79<'a, BinaryColor> {
    type Color = BinaryColor;
    type Error = Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        debug_assert!(matches!(self.inner.state.color_in_buf, ColorInBuf::Binary));
        for pixel in pixels {
            self.inner.set_binary(pixel);
        }
        Ok(())
    }
}

impl<'a> DrawTarget for Epd5in79<'a, Gray2> {
    type Color = Gray2;
    type Error = Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        debug_assert!(matches!(self.inner.state.color_in_buf, ColorInBuf::Gray));
        for pixel in pixels {
            self.inner.set_gray(pixel);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
enum ColorInBuf {
    Binary,
    Gray,
}

#[derive(Debug, Clone, Copy)]
struct Epd5in79State {
    power_on: Option<Instant>,
    color_in_buf: ColorInBuf,
    init_for: Option<DisplayMode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DisplayMode {
    Full,
    Fast,
    Partial,
    Gray2,
}

impl Epd5in79State {
    fn is_deepsleep(&self) -> bool {
        self.power_on.is_none()
    }

    fn check_deepsleep(&self) -> Result<(), anyhow::Error> {
        if self.is_deepsleep() {
            anyhow::bail!("epd is in deep sleep mode");
        }
        Ok(())
    }

    fn is_ready_for(&mut self, mode: DisplayMode) -> bool {
        (!self.is_deepsleep()) && self.init_for == Some(mode)
    }
}

fn is_point_in_screen(point: Point) -> bool {
    point.x >= 0 && point.x < 792 && point.y >= 0 && point.y < 272
}

#[rustfmt::skip]
static LUT_DATA: &[u8; 227] = &[
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,

    0x01, 0x4A, 0x00, 0x00, 0x00, 0x01, 0x00,
    0x01, 0x82, 0x42, 0x00, 0x00, 0x10, 0x00,
    0x01, 0x8A, 0x00, 0x00, 0x00, 0x01, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,

    0x01, 0x41, 0x00, 0x00, 0x00, 0x01, 0x00,
    0x01, 0x82, 0x42, 0x00, 0x00, 0x10, 0x00,
    0x01, 0x81, 0x00, 0x00, 0x00, 0x01, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,

    0x01, 0x81, 0x00, 0x00, 0x00, 0x01, 0x00,
    0x01, 0x82, 0x42, 0x00, 0x00, 0x10, 0x00,
    0x01, 0x41, 0x00, 0x00, 0x00, 0x01, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,

    0x01, 0x8A, 0x00, 0x00, 0x00, 0x01, 0x00,
    0x01, 0x82, 0x42, 0x00, 0x00, 0x10, 0x00,
    0x01, 0x4A, 0x00, 0x00, 0x00, 0x01, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,

    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,

    0x02, 0x00, 0x00
];

fn set_binary_value(color: BinaryColor, offset: u8, value: &mut u8) {
    if color.is_on() {
        *value |= 1 << offset;
    } else {
        *value &= !(1 << offset);
    }
}

fn set_gray_value(color: Gray2, offset: u8, bw_value: &mut u8, r_value: &mut u8) {
    match color.luma() {
        0 => {
            *bw_value &= !(1 << offset);
            *r_value &= !(1 << offset);
        }
        1 => {
            *bw_value |= 1 << offset;
            *r_value &= !(1 << offset);
        }
        2 => {
            *bw_value &= !(1 << offset);
            *r_value |= 1 << offset;
        }
        3 => {
            *bw_value |= 1 << offset;
            *r_value |= 1 << offset;
        }
        _ => unreachable!(),
    }
}

fn get_binary_from_value(offset: u8, value: u8) -> BinaryColor {
    if value & 1 << offset != 0 {
        BinaryColor::On
    } else {
        BinaryColor::Off
    }
}

fn get_gray_from_values(offset: u8, bw_value: u8, r_value: u8) -> Gray2 {
    match (bw_value & 1 << offset != 0, r_value & 1 << offset != 0) {
        (true, true) => Gray2::WHITE,
        (false, true) => Gray2::new(2),
        (true, false) => Gray2::new(1),
        (false, false) => Gray2::BLACK,
    }
}
