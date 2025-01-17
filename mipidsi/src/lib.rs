#![no_std]
// associated re-typing not supported in rust yet
#![allow(clippy::type_complexity)]
#![warn(missing_docs)]

//! This crate provides a generic display driver to connect to TFT displays
//! that implement the [MIPI Display Command Set](https://www.mipi.org/specifications/display-command-set).
//!
//! Uses [display_interface](https://crates.io/crates/display-interface) to talk to the hardware via transports.
//!
//! An optional batching of draws is supported via the `batch` feature (default on)
//!
//! ### List of supported models
//!
//! * ST7789
//! * ST7735
//! * ILI9486
//! * ILI9341
//! * ILI9342C
//!
//! ## Examples
//! **For the ili9486 display, using the SPI interface with no chip select:**
//! ```rust ignore
//! use display_interface_spi::SPIInterfaceNoCS;    // Provides the builder for DisplayInterface
//! use mipidsi::Builder;                           // Provides the builder for Display
//! use embedded_graphics::pixelcolor::Rgb666;      // Provides the required color type
//!
//! /* Define the SPI interface as the variable `spi` */
//! /* Define the DC digital output pin as the variable `dc` */
//! /* Define the Reset digital output pin as the variable `rst` */
//!
//! // Create a DisplayInterface from SPI and DC pin, with no manual CS control
//! let di = SPIInterfaceNoCS::new(spi, dc);
//!
//! // Create the ILI9486 display driver from the display interface and optional RST pin
//! let mut display = Builder::ili9486(di)
//!     .init(&mut delay, Some(rst)).unwrap();
//!
//! // Clear the display to black
//! display.clear(Rgb666::BLACK).unwrap();
//! ```
//!
//! **For the ili9341 display, using the Parallel port, with the RGB666 color space and the Bgr
//! color order:**
//! ```rust ignore
//! // Provides the builder for DisplayInterface
//! use display_interface_parallel_gpio::{Generic8BitBus, PGPIO8BitInterface};
//! // Provides the builder for Display
//! use mipidsi::Builder;
//! // Provides the required color type
//! use embedded_graphics::pixelcolor::Rgb666;
//!
//! /* Define digital output pins d0 - d7 for the parallel port as `lcd_dX` */
//! /* Define the D/C digital output pin as `dc` */
//! /* Define the WR and Reset digital output pins with the initial state set as High as `wr` and
//! `rst` */
//!
//! // Create the DisplayInterface from a Generic8BitBus, which is made from the parallel pins
//! let bus = Generic8BitBus::new((lcd_d0, lcd_d1, lcd_d2,
//!     lcd_d3, lcd_d4, lcd_d5, lcd_d6, lcd_d7)).unwrap();
//! let di = PGPIO8BitInterface::new(bus, dc, wr);
//!
//! // Create the ILI9341 display driver from the display interface with the RGB666 color space
//! let mut display = Builder::ili9341_rgb666(di)
//!      .with_color_order(mipidsi::ColorOrder::Bgr)
//!      .init(&mut delay, Some(rst)).unwrap();
//!
//! // Clear the display to black
//! display.clear(Rgb666::RED).unwrap();
//! ```
//! Use the appropiate display interface crate for your needs:
//! - [`display-interface-spi`](https://docs.rs/display-interface-spi/)
//! - [`display-interface-parallel-gpio`](https://docs.rs/display-interface-parallel-gpio)
//! - [`display-interface-i2c`](https://docs.rs/display-interface-i2c/)
//!
//! ## Troubleshooting
//! See [document](https://github.com/almindor/mipidsi/blob/master/docs/TROUBLESHOOTING.md)

use dcs::Dcs;
use display_interface::WriteOnlyDataCommand;

pub mod error;
use error::Error;

use embedded_hal::delay::DelayNs;
use embedded_hal::digital::OutputPin;

pub mod options;
use options::MemoryMapping;

mod builder;
pub use builder::Builder;

pub mod dcs;

pub mod models;
use models::Model;

mod graphics;

mod test_image;
pub use test_image::TestImage;

#[cfg(feature = "batch")]
mod batch;

///
/// Display driver to connect to TFT displays.
///
pub struct Display<DI, MODEL, RST>
where
    DI: WriteOnlyDataCommand,
    MODEL: Model,
    RST: OutputPin,
{
    // DCS provider
    dcs: Dcs<DI>,
    // Model
    model: MODEL,
    // Reset pin
    rst: Option<RST>,
    // Model Options, includes current orientation
    options: options::ModelOptions,
    // Current MADCTL value copy for runtime updates
    madctl: dcs::SetAddressMode,
    // State monitor for sleeping TODO: refactor to a Model-connected state machine
    sleeping: bool,
}

impl<DI, M, RST> Display<DI, M, RST>
where
    DI: WriteOnlyDataCommand,
    M: Model,
    RST: OutputPin,
{
    ///
    /// Returns currently set [options::Orientation]
    ///
    pub fn orientation(&self) -> options::Orientation {
        self.options.orientation()
    }

    ///
    /// Sets display [options::Orientation] with mirror image parameter
    ///
    /// # Example
    /// ```rust ignore
    /// display.orientation(Orientation::Portrait(false)).unwrap();
    /// ```
    pub fn set_orientation(&mut self, orientation: options::Orientation) -> Result<(), Error> {
        self.madctl = self.madctl.with_orientation(orientation); // set orientation
        self.dcs.write_command(self.madctl)?;

        Ok(())
    }

    ///
    /// Sets a pixel color at the given coords.
    ///
    /// # Arguments
    ///
    /// * `x` - x coordinate
    /// * `y` - y coordinate
    /// * `color` - the color value in pixel format of the display [Model]
    ///
    /// # Example
    /// ```rust ignore
    /// display.set_pixel(100, 200, Rgb666::new(251, 188, 20)).unwrap();
    /// ```
    pub fn set_pixel(&mut self, x: u16, y: u16, color: M::ColorFormat) -> Result<(), Error> {
        self.set_address_window(x, y, x, y)?;
        self.model
            .write_pixels(&mut self.dcs, core::iter::once(color))?;

        Ok(())
    }

    ///
    /// Sets pixel colors in a rectangular region.
    ///
    /// The color values from the `colors` iterator will be drawn to the given region starting
    /// at the top left corner and continuing, row first, to the bottom right corner. No bounds
    /// checking is performed on the `colors` iterator and drawing will wrap around if the
    /// iterator returns more color values than the number of pixels in the given region.
    ///
    /// This is a low level function, which isn't intended to be used in regular user code.
    /// Consider using the [`fill_contiguous`](https://docs.rs/embedded-graphics/latest/embedded_graphics/draw_target/trait.DrawTarget.html#method.fill_contiguous)
    /// function from the `embedded-graphics` crate as an alternative instead.
    ///
    /// # Arguments
    ///
    /// * `sx` - x coordinate start
    /// * `sy` - y coordinate start
    /// * `ex` - x coordinate end
    /// * `ey` - y coordinate end
    /// * `colors` - anything that can provide `IntoIterator<Item = u16>` to iterate over pixel data
    pub fn set_pixels<T>(
        &mut self,
        sx: u16,
        sy: u16,
        ex: u16,
        ey: u16,
        colors: T,
    ) -> Result<(), Error>
    where
        T: IntoIterator<Item = M::ColorFormat>,
    {
        self.set_address_window(sx, sy, ex, ey)?;
        self.model.write_pixels(&mut self.dcs, colors)?;

        Ok(())
    }

    /// Sets the vertical scroll region.
    ///
    /// The `top_fixed_area` and `bottom_fixed_area` arguments can be used to
    /// define an area on the top and/or bottom of the display which won't be
    /// affected by scrolling.
    ///
    /// Note that this method is not affected by the current display orientation
    /// and will always scroll vertically relative to the default display
    /// orientation.
    ///
    /// The combined height of the fixed area must not larger than the
    /// height of the framebuffer height in the default orientation.
    ///
    /// After the scrolling region is defined the [`set_scroll_offset`] can be
    /// used to scroll the display.
    pub fn set_vertical_scroll_region(
        &mut self,
        top_fixed_area: u16,
        bottom_fixed_area: u16,
    ) -> Result<(), Error> {
        let rows = M::FRAMEBUFFER_SIZE.1;

        let vscrdef = if top_fixed_area + bottom_fixed_area > rows {
            dcs::SetScrollArea::new(rows, 0, 0)
        } else {
            dcs::SetScrollArea::new(
                top_fixed_area,
                rows - top_fixed_area - bottom_fixed_area,
                bottom_fixed_area,
            )
        };

        self.dcs.write_command(vscrdef)
    }

    /// Sets the vertical scroll offset.
    ///
    /// Setting the vertical scroll offset shifts the vertical scroll region
    /// upwards by `offset` pixels.
    ///
    /// Use [`set_vertical_scroll_region`] to setup the scroll region, before
    /// using this method.
    pub fn set_vertical_scroll_offset(&mut self, offset: u16) -> Result<(), Error> {
        let vscad = dcs::SetScrollStart::new(offset);
        self.dcs.write_command(vscad)
    }

    ///
    /// Release resources allocated to this driver back.
    /// This returns the display interface, reset pin and and the model deconstructing the driver.
    ///
    pub fn release(self) -> (DI, M, Option<RST>) {
        (self.dcs.release(), self.model, self.rst)
    }

    // Sets the address window for the display.
    fn set_address_window(&mut self, sx: u16, sy: u16, ex: u16, ey: u16) -> Result<(), Error> {
        // add clipping offsets if present
        let mut offset = self.options.display_offset;
        let mapping = MemoryMapping::from(self.options.orientation);
        if mapping.reverse_columns {
            offset.0 = M::FRAMEBUFFER_SIZE.0 - (self.options.display_size.0 + offset.0);
        }
        if mapping.reverse_rows {
            offset.1 = M::FRAMEBUFFER_SIZE.1 - (self.options.display_size.1 + offset.1);
        }
        if mapping.swap_rows_and_columns {
            offset = (offset.1, offset.0);
        }

        let (sx, sy, ex, ey) = (sx + offset.0, sy + offset.1, ex + offset.0, ey + offset.1);

        self.dcs.write_command(dcs::SetColumnAddress::new(sx, ex))?;
        self.dcs.write_command(dcs::SetPageAddress::new(sy, ey))
    }

    ///
    /// Configures the tearing effect output.
    ///
    pub fn set_tearing_effect(
        &mut self,
        tearing_effect: options::TearingEffect,
    ) -> Result<(), Error> {
        self.dcs
            .write_command(dcs::SetTearingEffect::new(tearing_effect))
    }

    ///
    /// Returns `true` if display is currently set to sleep.
    ///
    pub fn is_sleeping(&self) -> bool {
        self.sleeping
    }

    ///
    /// Puts the display to sleep, reducing power consumption.
    /// Need to call [Self::wake] before issuing other commands
    ///
    pub fn sleep<D: DelayNs>(&mut self, delay: &mut D) -> Result<(), Error> {
        self.dcs.write_command(dcs::EnterSleepMode)?;
        // All supported models requires a 120ms delay before issuing other commands
        delay.delay_us(120_000);
        self.sleeping = true;
        Ok(())
    }

    ///
    /// Wakes the display after it's been set to sleep via [Self::sleep]
    ///
    pub fn wake<D: DelayNs>(&mut self, delay: &mut D) -> Result<(), Error> {
        self.dcs.write_command(dcs::ExitSleepMode)?;
        // ST7789 and st7735s have the highest minimal delay of 120ms
        delay.delay_us(120_000);
        self.sleeping = false;
        Ok(())
    }

    /// Returns the DCS interface for sending raw commands.
    ///
    /// # Safety
    ///
    /// Sending raw commands to the controller can lead to undefined behaviour,
    /// because the rest of the code isn't aware of any state changes that were caused by sending raw commands.
    /// The user must ensure that the state of the controller isn't altered in a way that interferes with the normal
    /// operation of this crate.
    pub unsafe fn dcs(&mut self) -> &mut Dcs<DI> {
        &mut self.dcs
    }
}
