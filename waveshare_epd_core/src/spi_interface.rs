//! Defined a common SPI interface.
//!
//! Considering generality, this interface uses [embedded-hal](https://docs.rs/embedded-hal/latest/embedded_hal/).
//!
//! This also requires additional configuration of certain pins to ensure correct behavior.
//!
//! # Conventions:
//! - `dc_pin`: Low level for command, high level for data
//! - `cs_pin`: Low level for active (ACTIVE_LOW)

use std::{
    fmt::Debug,
    marker::PhantomData,
    time::{Duration, Instant},
};

use crate::error::TimeOutError;
use embedded_hal::{
    delay::DelayNs,
    digital::{InputPin, OutputPin},
    spi::SpiDevice,
};

/// A common SPI interface uses [embedded-hal](https://docs.rs/embedded-hal/latest/embedded_hal/).
pub struct SpiInterface<Spi, I, O, D, E> {
    spi: Spi,
    rst_pin: O,
    dc_pin: O,
    cs_pin: Option<O>,
    busy_pin: I,
    pwr_pin: O,

    delay: D,

    marker: PhantomData<E>,
}

impl<Spi, I, O, D, E> Debug for SpiInterface<Spi, I, O, D, E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpiInterface").finish_non_exhaustive()
    }
}

impl<Spi, I, O, D, E> SpiInterface<Spi, I, O, D, E>
where
    Spi: SpiDevice,
    I: InputPin,
    O: OutputPin,
    D: DelayNs,
    E: From<Spi::Error> + From<I::Error> + From<O::Error>,
{
    pub fn new(
        spi: Spi,
        rst_pin: O,
        dc_pin: O,
        cs_pin: Option<O>,
        busy_pin: I,
        pwr_pin: O,

        delay: D,
    ) -> Self {
        Self {
            spi,
            rst_pin,
            dc_pin,
            cs_pin,
            busy_pin,
            pwr_pin,
            delay,
            marker: PhantomData,
        }
    }

    fn set_cs(&mut self, active: bool) -> Result<(), E> {
        if let Some(cs) = self.cs_pin.as_mut() {
            if active {
                cs.set_high()?;
            } else {
                cs.set_low()?;
            }
        }
        Ok(())
    }

    pub fn is_busy(&mut self) -> Result<bool, E> {
        Ok(self.busy_pin.is_high()?)
    }

    pub fn set_rst_pin(&mut self, active: bool) -> Result<(), E> {
        if active {
            self.rst_pin.set_high()?;
        } else {
            self.rst_pin.set_low()?;
        }
        Ok(())
    }

    pub fn delay(&mut self, delay: DelayStep) {
        match delay {
            DelayStep::Ms(ms) => self.delay.delay_ms(ms),
            DelayStep::Us(us) => self.delay.delay_us(us),
            DelayStep::Ns(ns) => self.delay.delay_ns(ns),
        }
    }

    pub fn wait_busy_timeout(&mut self, delay: DelayStep, timeout: Duration) -> Result<Duration, E>
    where
        E: From<TimeOutError>,
    {
        let now = Instant::now();
        if !self.is_busy()? {
            return Ok(now.elapsed());
        }

        let delay = delay.max_one();
        while now.elapsed() < timeout {
            self.delay(delay);
            if !self.is_busy()? {
                return Ok(now.elapsed());
            }
        }

        Err(TimeOutError {
            timeout,
            elapsed: now.elapsed(),
        }
        .into())
    }

    pub fn command(&mut self, cmd: u8) -> Result<(), E> {
        self.set_cs(true)?;
        self.dc_pin.set_low()?;
        self.spi.write(&[cmd])?;
        self.set_cs(false)?;
        Ok(())
    }

    pub fn data(&mut self, data: impl AsRef<[u8]>, chunk_size: usize) -> Result<(), E> {
        let data = data.as_ref();
        let chunk_size = chunk_size.max(1);
        if data.is_empty() {
            return Ok(());
        }
        self.set_cs(true)?;
        self.dc_pin.set_high()?;
        for chunk in data.chunks(chunk_size) {
            self.spi.write(chunk)?;
        }
        self.set_cs(false)?;
        Ok(())
    }

    pub fn command_data(
        &mut self,
        cmd: u8,
        data: impl AsRef<[u8]>,
        chunk_size: usize,
    ) -> Result<(), E> {
        self.command(cmd)?;
        self.data(data, chunk_size)?;
        Ok(())
    }

    pub fn set_power(&mut self, on: bool) -> Result<(), E> {
        if on {
            self.pwr_pin.set_high()?;
        } else {
            self.pwr_pin.set_low()?;
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct PinDefinition {
    pub rst_pin: u32,
    pub dc_pin: u32,
    pub cs_pin: Option<u32>,
    pub busy_pin: u32,
    pub pwr_pin: u32,
}

impl PinDefinition {
    /// Default without `cs_pin`.
    pub const DEFAULT: PinDefinition = PinDefinition::new(17, 25, None, 24, 18);
    pub const DEFAULT_WITH_CS: PinDefinition = PinDefinition::new(17, 25, Some(8), 24, 18);

    pub const fn new(
        rst_pin: u32,
        dc_pin: u32,
        cs_pin: Option<u32>,
        busy_pin: u32,
        pwr_pin: u32,
    ) -> Self {
        Self {
            rst_pin,
            dc_pin,
            cs_pin,
            busy_pin,
            pwr_pin,
        }
    }
}

impl Default for PinDefinition {
    /// Default without `cs_pin`.
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[derive(Debug, Clone, Copy)]
pub enum DelayStep {
    Ns(u32),
    Us(u32),
    Ms(u32),
}

impl DelayStep {
    fn max_one(self) -> Self {
        match self {
            Self::Ns(ns) => Self::Ns(ns.max(1)),
            Self::Us(us) => Self::Us(us.max(1)),
            Self::Ms(ms) => Self::Ms(ms.max(1)),
        }
    }
}
