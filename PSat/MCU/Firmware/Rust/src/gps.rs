#![allow(dead_code)]

use core::{fmt::Debug, num::ParseIntError};

use arrayvec::{ArrayString, ArrayVec};
use fixed::{types::extra::{U10, U24}, FixedI32};
use msp430fr2x5x_hal::{
    clock::Smclk, 
    serial::{BitCount, BitOrder, Loopback, Parity, RecvError, SerialConfig, StopBits}};
use embedded_hal::serial::Read;
use ufmt::{derive::uDebug, uDisplay, uwrite};
use crate::pin_mappings::{GpsEusci, GpsRx, GpsRxPin, GpsTx, GpsTxPin};

const NMEA_MESSAGE_MAX_LEN: usize = 82;

pub struct Gps {
    tx: GpsTx,
    rx: GpsRx,
}
impl Gps {
    pub fn new(eusci_reg: GpsEusci, smclk: &Smclk, tx_pin: GpsTxPin, rx_pin: GpsRxPin) -> Self {
        // Configure UART peripheral
        let (tx, rx) = SerialConfig::new(eusci_reg, 
            BitOrder::LsbFirst, 
            BitCount::EightBits, 
            StopBits::OneStopBit, 
            Parity::NoParity, 
            Loopback::NoLoop, 
            9600)
            .use_smclk(smclk)
            .split(tx_pin, rx_pin);
        Self {tx, rx}
    } 

    /// Get the next GPS packet as an ArrayString.
    pub fn get_nmea_string_message_blocking(&mut self) -> Result<ArrayString<NMEA_MESSAGE_MAX_LEN>, RecvError> {
        let mut msg = ArrayString::new();
        // Wait until start of a message. Messages begin with '$'
        while nb::block!(self.rx.read())? != b'$' {}
        msg.push('$');

        // Push chars until '\n'
        loop {
            let chr = nb::block!(self.rx.read())?;
            msg.push(chr as char);
            if chr == b'\n' { break; }
        }

        Ok(msg)
    }
    /// Get a GPS GGA packet as an ArrayString. Useful if you're just sending over the radio or logging to an SD card.
    pub fn get_gga_string_message_blocking(&mut self) -> Result<ArrayString<NMEA_MESSAGE_MAX_LEN>, RecvError> {
        loop {
            let msg = self.get_nmea_string_message_blocking()?;
            if &msg[3..6] == "GGA" { return Ok(msg) }
        }
    }
    /// Get a GPS GGA packet as a struct. Useful for on-device computation.
    pub fn get_gga_message_blocking(&mut self) -> Result<GgaMessage, GgaParseError> {
        let msg = self.get_gga_string_message_blocking().map_err(GgaParseError::SerialError)?;
        crate::println!("{}", msg.as_str());
        GgaMessage::try_from(msg)
    }
}

// A GGA packet in struct form. Useful for interpreting the results on-device.
pub struct GgaMessage {
    pub utc_time: UtcTime,
    pub latitude: Degrees,
    pub longitude: Degrees,
    pub fix_type: GpsFixType,
    pub num_satellites: u8,
    pub altitude_msl: Altitude,
}
impl TryFrom<ArrayString<NMEA_MESSAGE_MAX_LEN>> for GgaMessage {
    type Error = GgaParseError;

    fn try_from(msg: ArrayString<NMEA_MESSAGE_MAX_LEN>) -> Result<Self, Self::Error> {
        let sections: ArrayVec<&str, 15> = msg.split(',').take(15).collect();
        if sections.len() != 15 { return Err(GgaParseError::WrongSectionCount) }

        let fix_type = GpsFixType::try_from(sections[6]).map_err(|_| GgaParseError::InvalidGpsFixType)?;
        if fix_type == GpsFixType::None { return Err(GgaParseError::NoFix) }

        Ok( GgaMessage { 
            utc_time: UtcTime::try_from(sections[1])                .unwrap(),//.map_err(GgaParseError::UtcParseError)?, 
            latitude: Degrees::try_from((sections[2], sections[3])) .unwrap(),//.map_err(GgaParseError::LatLongParseError)?, 
            longitude: Degrees::try_from((sections[4], sections[5])).unwrap(),//.map_err(GgaParseError::LatLongParseError)?, 
            num_satellites: sections[7].parse()                     .unwrap(),//.map_err(GgaParseError::InvalidSatelliteNumber)?, 
            altitude_msl: Altitude::try_from(sections[9])           .unwrap(),//.map_err(GgaParseError::AltitudeParseError)?, 
            fix_type,
        })
    }
}

pub enum GgaParseError {
    NoFix,
    SerialError(RecvError),
    WrongSectionCount,
    LatLongParseError(LatLongParseError),
    InvalidGpsFixType,
    InvalidSatelliteNumber(ParseIntError),
    UtcParseError(UtcError),
    AltitudeParseError(ParseIntError),
}
impl Debug for GgaParseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::SerialError(arg0) => {
                let str = match arg0 {
                    RecvError::Framing => "Framing",
                    RecvError::Parity => "Parity",
                    RecvError::Overrun(_) => "Overrun",
                };
                f.debug_tuple("SerialError").field(&str).finish()
            }
            e => write!(f, "{:?}", e),
        }
    }
}

/// A UTC timestamp
pub struct UtcTime {
    pub hours: u8,
    pub minutes: u8,
    pub seconds: u8,
    pub millis: u16, 
}
impl uDisplay for UtcTime {
    fn fmt<W>(&self, f: &mut ufmt::Formatter<'_, W>) -> Result<(), W::Error>
    where
        W: ufmt::uWrite + ?Sized {
        for time in [self.hours, self.minutes] {
            match time {
                0..10 => uwrite!(f, "0{}:", time)?,
                10.. =>  uwrite!(f,  "{}:", time)?,
            };
        }

        match self.seconds {
            0..10 =>  uwrite!(f, "0{}.", self.seconds)?,
            10..  =>  uwrite!(f,  "{}.", self.seconds)?,
        };

        match self.millis {
            0..10   => uwrite!(f, "00{} UTC", self.millis)?,
            10..100 => uwrite!(f,  "0{} UTC", self.millis)?,
            100..   => uwrite!(f,   "{} UTC", self.millis)?,
        };

        Ok(())
    }
}
impl TryFrom<&str> for UtcTime {
    type Error = UtcError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        crate::println!("{}", value);
        if value.len() < 6 {
            return Err(UtcError::StrTooShort)
        }
        Ok(UtcTime { 
            hours: value[0..2].parse().map_err(UtcError::ParseError)?, 
            minutes: value[2..4].parse().map_err(UtcError::ParseError)?, 
            seconds: value[4..6].parse().map_err(UtcError::ParseError)?, 
            millis: value.get(7..).unwrap_or("0").parse().map_err(UtcError::ParseError)? })
    }
}
#[derive(Debug)]
pub enum UtcError {
    StrTooShort,
    ParseError(ParseIntError),
}

/// A degrees value, stored as a decimal fraction.
pub struct Degrees(pub FixedI32<U24>);
impl uDisplay for Degrees {
    fn fmt<W>(&self, f: &mut ufmt::Formatter<'_, W>) -> Result<(), W::Error>
    where
        W: ufmt::uWrite + ?Sized {
        const FRAC_BITS: u32 = 24;
        const FRAC_MASK: u32 = (1 << FRAC_BITS) - 1;
        const MAX_PRECISION: u32 = 6;

        let mut fxd = self.0;
            
        let sign = if fxd < 0 {'-'} else {'+'};

        // Fractional bits are always positive, even for negative numbers. Make the number positive so they make sense
        if sign == '-' {fxd *= -1;} 

        let whole: i32 = fxd.to_num();
        uwrite!(f, "{}{}", sign, whole)?;

        let mut frac: u32 = (fxd.frac().to_bits() as u32) & FRAC_MASK;

        if frac != 0 { uwrite!(f, ".")?; }

        let mut precision = 0;
        while frac != 0 && precision < MAX_PRECISION {
            frac *= 10;
            let digit = frac >> FRAC_BITS;
            uwrite!(f, "{}", digit)?;
            frac &= FRAC_MASK;
            precision += 1;
        }
        uwrite!(f, "°")
    }
}
impl TryFrom<(&str, &str)> for Degrees {
    type Error = LatLongParseError;

    fn try_from(value: (&str, &str)) -> Result<Self, Self::Error> {
        let (degrees_str, compass_direction) = value;
        crate::println!("{}, {}", degrees_str, compass_direction);
        if degrees_str.is_empty() || compass_direction.is_empty() {
            return Err(LatLongParseError::NoData);
        }
        let degrees: i16; 
        let minutes: FixedI32::<U24>;
        let (first_half, _) = degrees_str.split_once('.').unwrap();
    
        if first_half.len() == 4 { // ddmm
            degrees = degrees_str[0..2].parse().unwrap();
            minutes = degrees_str[2.. ].parse().unwrap();
        } else { // dddmm
            degrees = degrees_str[0..3].parse().unwrap();
            minutes = degrees_str[3.. ].parse().unwrap();
        }
    
        let fxd = FixedI32::<U24>::from_num(degrees) + minutes/60;
    
        match compass_direction {
            "N" | "E" => Ok(Degrees(fxd)),
            "S" | "W" => Ok(Degrees(-fxd)),
            _ => Err(LatLongParseError::InvalidCompassDirection)
        }
    }
}
#[derive(Debug)]
pub enum LatLongParseError {
    NoData,
    InvalidCompassDirection,
}

#[derive(Debug, uDebug, PartialEq, Eq)]
pub enum GpsFixType {
    None = 0,
    Gps = 1,
    DifferentialGps = 2,
}
impl TryFrom<&str> for GpsFixType{
    type Error = ();

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        crate::println!("{}", value);
        Ok(match value {
            "0" => GpsFixType::None,
            "1" => GpsFixType::Gps,
            "2" => GpsFixType::DifferentialGps,
            _ => return Err(()), // should be unreachable
        })
    }
}

pub struct Altitude(pub FixedI32<U10>);
impl TryFrom<&str> for Altitude {
    type Error = fixed::ParseFixedError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        crate::println!("{}", value);
        Ok(Altitude(value.parse()?))
    }
}
impl uDisplay for Altitude {
    fn fmt<W>(&self, f: &mut ufmt::Formatter<'_, W>) -> Result<(), W::Error>
    where
        W: ufmt::uWrite + ?Sized { 
        const FRAC_BITS: u32 = 10;
        const FRAC_MASK: u32 = (1 << FRAC_BITS) - 1;
        const MAX_PRECISION: u32 = 3;

        let mut fxd = self.0;
            
        let sign = if fxd < 0 {'-'} else {'+'};

        // Fractional bits are always positive, even for negative numbers. Make the number positive so they make sense
        if sign == '-' {fxd *= -1;} 

        let whole: i32 = fxd.to_num();
        uwrite!(f, "{}{}", sign, whole)?;

        let mut frac: u32 = (fxd.frac().to_bits() as u32) & FRAC_MASK;

        if frac != 0 { uwrite!(f, ".")?; }

        let mut precision = 0;
        while frac != 0 && precision < MAX_PRECISION {
            frac *= 10;
            let digit = frac >> FRAC_BITS;
            uwrite!(f, "{}", digit)?;
            frac &= FRAC_MASK;
            precision += 1;
        }
        uwrite!(f, "m")
    }
}