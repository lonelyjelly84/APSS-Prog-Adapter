#![allow(dead_code)]
use arrayvec::ArrayVec;
use embedded_hal_bus::spi::ExclusiveDevice;
use embedded_lora_rfm95::{error::{IoError, RxCompleteError, TxStartError}, lora::types::{Bandwidth, CodingRate, CrcMode, HeaderMode, Polarity, PreambleLength, SpreadingFactor, SyncWord}, rfm95::{self, Rfm95Driver}};
use embedded_hal_compat::{eh1_0::delay::DelayNs, markers::ForwardOutputPin, Forward, ForwardCompat};
use msp430fr2x5x_hal::{delay::Delay, gpio::{Output, Pin, Pin4}, spi::SpiBus, pac::P4};
use crate::pin_mappings::{RadioCsPin, RadioEusci, RadioResetPin, RadioSpi};

const LORA_FREQ_HZ: u32 = 915_000_000;

pub fn new(spi: RadioSpi, cs_pin: RadioCsPin, reset_pin: RadioResetPin, delay: Delay) -> Radio {
    let device: ExclusiveDeviceType = embedded_hal_bus::spi::ExclusiveDevice::new(spi.forward(), cs_pin.forward(), DelayWrapper(delay)).unwrap();
    let mut rfm95 = match Rfm95Driver::<ExclusiveDeviceType>::new_from_device(device, reset_pin.forward(), &mut DelayWrapper(delay)) {
        Ok(rfm) => rfm,
        Err(_e) => panic!("Radio reports invalid silicon revision. Is the beacon connected?"),
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
    rfm95.set_config(&lora_config).unwrap_or_else(|_| unreachable!()); // Compat layer doesn't support Debug :(

    Radio{driver: rfm95, rx_idle: false, tx_idle: false}
}

type ForwardSpiBus = Forward<SpiBus<RadioEusci>, ()>;
type ForwardCsPin = Forward<Pin<P4, Pin4, Output>, ForwardOutputPin>;
type ExclusiveDeviceType = ExclusiveDevice<ForwardSpiBus, ForwardCsPin, DelayWrapper>;
type RFM95 = Rfm95Driver<ExclusiveDeviceType>;
/// Top-level interface for the radio module.
pub struct Radio {
    pub driver: RFM95,
    rx_idle: bool,
    tx_idle: bool,
}
impl Radio {
    // TODO: Test
    pub fn recieve(&mut self, buf: &mut ArrayVec<u8, { rfm95::RFM95_FIFO_SIZE }>) -> nb::Result<(), RxError> {
        if self.rx_idle {
            let timeout = self.driver.rx_timeout_max().unwrap();
            self.driver.start_rx(timeout).unwrap();
            self.rx_idle = false;
        }

        match self.driver.complete_rx(buf) {
            Ok(None) => Err(nb::Error::WouldBlock),
            Ok(Some(_n)) => {
                self.rx_idle = true;
                Ok(())
            },
            Err(RxCompleteError::InvalidMessageError(_)) => Err(nb::Error::Other(RxError::CrcFailure)),
            Err(RxCompleteError::TimeoutError(_)) => {
                let timeout = self.driver.rx_timeout_max().unwrap();
                self.driver.start_rx(timeout).unwrap();
                Err(nb::Error::WouldBlock)
            },
            Err(RxCompleteError::IoError(_)) => unreachable!()
        }
    }

    // TODO: Test
    /// On the first invocation `data` is copied into the radio's buffer. Changes to `data` will have no effect until the method returns `Ok()` and the method is called again.
    pub fn transmit(&mut self, data: &[u8]) -> nb::Result<usize, TxError> {
        if self.tx_idle {
            match self.driver.start_tx(data) {
                Ok(_) => (),
                Err(TxStartError::InvalidArgumentError(_)) => Err(nb::Error::Other(TxError::InvalidBufferSize))?,
                Err(TxStartError::IoError(_)) => unreachable!(),
            };
            self.tx_idle = false;
        }

        match self.driver.complete_tx() {
            Ok(None) => Err(nb::Error::WouldBlock),  // Still sending
            Ok(Some(n)) => {                         // Sending complete
                self.tx_idle = true;
                Ok(n)
            },
            Err(IoError{}) => unreachable!(),
        }
    }
}

pub enum RxError {
    CrcFailure,
}

pub enum TxError {
    InvalidBufferSize,
}

use embedded_hal::blocking::delay::DelayMs;
// The radio library uses a different version of embedded_hal, so we need to write some wrappers.
pub struct DelayWrapper(Delay);
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
            if board.radio.recieve(&mut buf).is_ok() {
                // let Ok(signal_strength) = board.radio.driver.get_packet_strength() else {continue};
                // let Ok(rssi) = board.radio.driver.get_rssi() else {continue};
                // let Ok(snr) = board.radio.driver.get_packet_snr() else {continue};
                //crate::println!("[{}] '{}', Strength: {}, RSSI: {}, SNR: {}", current_time, core::str::from_utf8(&buf).unwrap(), signal_strength, rssi, snr);
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