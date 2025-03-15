#![allow(dead_code)]
use arrayvec::ArrayVec;
use embedded_lora_rfm95::{lora::types::{Bandwidth, CodingRate, CrcMode, HeaderMode, Polarity, PreambleLength, RxConfigError, SingleRxError, SpiError, SpreadingFactor, SyncWord, TxError}, rfm95::{self, Rfm95Driver}};
use embedded_hal_compat::{eh1_0::delay::DelayNs, Forward, ForwardCompat};
use msp430fr2x5x_hal::{delay::Delay, gpio::{Output, Pin, Pin4}, spi::SpiBus, pac::P4};
use crate::pin_mappings::{RadioCsPin, RadioEusci, RadioResetPin, RadioSpi};

const LORA_FREQ_HZ: u32 = 915_000_000;

pub fn new(spi: RadioSpi, cs_pin: RadioCsPin, reset_pin: RadioResetPin, delay: Delay) -> Result<Radio, SpiError<FwSpiBus, FwSelectPin>> {
    let mut rfm95 = match Rfm95Driver::new(spi.forward(), cs_pin.forward(), reset_pin.forward(), DelayWrapper(delay)) {
        Ok(rfm) => rfm,
        Err(embedded_lora_rfm95::lora::types::InitError::ResetPin(_e)) => unreachable!(),
        Err(embedded_lora_rfm95::lora::types::InitError::UnsupportedSiliconRevision(_ver)) => todo!(),
        Err(embedded_lora_rfm95::lora::types::InitError::Spi(e)) => Err(e)?,
    };

    // 62.5kHz bandwidth, 4/5 coding rate, SF10 gives a bitrate of about 500bps.
    let lora_config = embedded_lora_rfm95::lora::config::Builder::builder()
        .set_bandwidth(Bandwidth::B62_5) // lower bandwidth == longer range, but very low bandwidths can suffer from clock source tolerance issues
        .set_coding_rate(CodingRate::C4_5) // Error correction lowers bitrate. Consider how electronically noisy the area might be.
        .set_crc_mode(CrcMode::Disabled)
        .set_frequency(LORA_FREQ_HZ.into())
        .set_header_mode(HeaderMode::Explicit)
        .set_polarity(Polarity::Normal)
        .set_preamble_length(PreambleLength::L8)
        .set_spreading_factor(SpreadingFactor::S10) // High SF == Best range
        .set_sync_word(SyncWord::PRIVATE);
    rfm95.set_config(&lora_config)?;

    Ok( Radio{driver: rfm95, rx_idle: false, tx_idle: false} )
}

type FwSpiBus = Forward<SpiBus<RadioEusci>>; 
type FwSelectPin = Forward<Pin<P4, Pin4, Output>, embedded_hal_compat::markers::ForwardOutputPin>;
pub type RFM95 = Rfm95Driver<FwSpiBus, FwSelectPin>;
/// Top-level interface for the radio module.
pub struct Radio {
    pub driver: RFM95,
    rx_idle: bool,
    tx_idle: bool,
}
impl Radio {
    // TODO: Test
    pub fn recieve(&mut self, buf: &mut ArrayVec<u8, { rfm95::RFM95_FIFO_SIZE }>) -> nb::Result<(), SpiError<FwSpiBus, FwSelectPin>> {
        if self.rx_idle {
            let timeout = self.driver.rx_timeout_max()?;
            if let Err(RxConfigError::Spi(e)) = self.driver.start_rx(timeout) { Err(e)? }
            self.rx_idle = false;
        }
        match self.driver.complete_rx(buf) {
            Ok(Some(_)) => {
                self.rx_idle = true;
                Ok(())
            },
            Ok(None) => Err(nb::Error::WouldBlock),
            Err(embedded_lora_rfm95::lora::types::SingleRxError::RxTimeout) => {
                let timeout = self.driver.rx_timeout_max()?;
                if let Err(RxConfigError::Spi(e)) = self.driver.start_rx(timeout) { Err(e)? }
                Err(nb::Error::WouldBlock)
            },
            Err(SingleRxError::CrcFailure) => todo!(),
            Err(SingleRxError::Spi(e)) => Err(e)?,
        }
    }

    // TODO: Test
    pub fn transmit(&mut self, data: &[u8]) -> nb::Result<usize, TxError<FwSpiBus, FwSelectPin>> {
        if self.tx_idle {
            self.driver.start_tx(data)?;
            self.tx_idle = false;
        }

        match self.driver.complete_tx() {
            Ok(None) => Err(nb::Error::WouldBlock),  // Still sending
            Ok(Some(n)) => {                         // Sending complete
                self.tx_idle = true;
                Ok(n)
            },
            Err(e) => Err(TxError::from(e))?,
        }
    }
}

use embedded_hal::blocking::delay::DelayMs;
// The radio library uses a different version of embedded_hal, so we need to write some wrappers.
struct DelayWrapper(Delay);
impl DelayNs for DelayWrapper {
    fn delay_ms(&mut self, ms: u32) {
        if ms < (u16::MAX as u32) {
            self.0.delay_ms(ms as u16);
        }
        else {
            let times = ms/(u16::MAX as u32);

            for _ in 0..times {
                self.0.delay_ms(u16::MAX);
            }
            let remainder = ms - times*(u16::MAX as u32);
            self.0.delay_ms(remainder as u16);
        }
    }
    
    fn delay_ns(&mut self, ns: u32) {
        let ms = ns / 1_000_000;
        self.0.delay_ms(ms as u16);
    }
}

pub mod tests {
    use arrayvec::ArrayVec;
    use embedded_hal::timer::CountDown;
    use ufmt::uwrite;

    pub fn range_test_tx(mut board: crate::board::Board) -> ! {
        let mut current_time = Time::default();
        board.timer_b0.start(msp430fr2x5x_hal::clock::REFOCLK); // 1 second timer
        loop {
            let bytes = [
                current_time.hours / 10 + b'0', 
                current_time.hours % 10 + b'0', 
                b':', 
                current_time.minutes / 10 + b'0', 
                current_time.minutes % 10 + b'0', 
                b':', 
                current_time.seconds / 10 + b'0', 
                current_time.seconds % 10 + b'0'];
            let _ = board.radio.transmit(bytes.as_slice());
            nb::block!(board.timer_b0.wait()).ok();
            current_time.increment();
            board.gpio.green_led.toggle();
        }
    }

    pub fn range_test_rx(mut board: crate::board::Board) -> ! {
        let mut buf = ArrayVec::new();
        let mut current_time = Time::default();
        board.timer_b0.start(msp430fr2x5x_hal::clock::REFOCLK); // 1 second timer
        loop {
            match board.radio.recieve(&mut buf) {
                Ok(_) => {
                    let Ok(signal_strength) = board.radio.driver.get_packet_strength() else {continue};
                    let Ok(rssi) = board.radio.driver.get_rssi() else {continue};
                    let Ok(snr) = board.radio.driver.get_packet_snr() else {continue};
                    crate::println!("[{}] '{}', Strength: {}, RSSI: {}, SNR: {}", current_time, core::str::from_utf8(&buf).unwrap(), signal_strength, rssi, snr);
                },
                Err(nb::Error::Other(_e)) => (),
                Err(nb::Error::WouldBlock) => (),
            }

            if board.timer_b0.wait().is_ok() {
                current_time.increment();
            }
        }
    }
    #[derive(Default)]
    struct Time {
        seconds: u8,
        minutes: u8,
        hours: u8,
    }
    impl Time {
        /// Add one second to the time.
        pub fn increment(&mut self) {
            if self.seconds < 59 {
                self.seconds += 1;
                return;
            }

            self.seconds = 0;
            if self.minutes < 59 {
                self.minutes += 1;
                return;
            }

            self.minutes = 0;
            self.hours += 1;
        }
    }
    impl ufmt::uDisplay for Time {
        fn fmt<W: ufmt::uWrite + ?Sized>(&self, f: &mut ufmt::Formatter<'_, W>) -> Result<(), W::Error> {
            for (i, &val) in [self.hours, self.minutes, self.seconds].iter().enumerate() {
                if val < 10 {
                    ufmt::uwrite!(f, "0{}", val)?;
                }
                else {
                    ufmt::uwrite!(f, "{}", val)?; 
                }
                if i != 2 {
                    ufmt::uwrite!(f, ":")?;
                }
            }
            Ok(())
        }
    }
}