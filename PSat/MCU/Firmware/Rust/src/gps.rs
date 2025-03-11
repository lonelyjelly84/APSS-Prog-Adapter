#![allow(dead_code)]

use core::{fmt::Debug, num::ParseIntError};

use arrayvec::ArrayString;
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

/// A degrees value, stored as a decimal fraction.
pub struct Degrees {
    pub degrees: i16,
    pub frac: u32,
}
impl uDisplay for Degrees {
    fn fmt<W>(&self, f: &mut ufmt::Formatter<'_, W>) -> Result<(), W::Error>
    where
        W: ufmt::uWrite + ?Sized {
        const MAX_LEN: usize = u32::MAX.ilog10() as usize;
        let mut padding: ArrayString<MAX_LEN> = ArrayString::new();
        for _ in 0..MAX_LEN-self.frac.ilog10() as usize { padding.push('0'); }
        uwrite!(f, "{}.{}{}°", self.degrees, padding.as_str(), self.frac)
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
        let minutes: u8;
        let minutes_frac: u8;
        let (first_half, _) = degrees_str.split_once('.').unwrap();
    
        if first_half.len() == 4 { // ddmm
            degrees = degrees_str[0..2].parse().unwrap();
            minutes = degrees_str[2..4].parse::<u8>().unwrap() / 60;
            minutes_frac = degrees_str[4..].parse::<u8>().unwrap() / 60;
        } else { // dddmm
            degrees = degrees_str[0..3].parse().unwrap();
            minutes = degrees_str[3..5].parse::<u8>().unwrap() / 60;
            minutes_frac = degrees_str[5..].parse::<u8>().unwrap() / 60;
        }
    
        let frac: u32 = (minutes as u32*100_000 + minutes_frac as u32) / 60;
    
        match compass_direction {
            "N" | "E" => Ok(Degrees{degrees, frac}),
            "S" | "W" => Ok(Degrees{degrees: -degrees, frac}),
            _ => Err(LatLongParseError::InvalidCompassDirection)
        }
    }
}
#[derive(Debug)]
pub enum LatLongParseError {
    NoData,
    InvalidCompassDirection,
}

// A GGA packet in native form. Useful for interpreting the results on-device.
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
        let mut sections = msg.split(',');
        let num_sections = sections.clone().count();
        if num_sections != 15 { return Err(GgaParseError::WrongSectionCount) } 
        let fix_type = GpsFixType::try_from(sections.clone().nth(6).unwrap()).map_err(|_| GgaParseError::InvalidGpsFixType)?;
        if fix_type == GpsFixType::None { return Err(GgaParseError::NoFix) }

        Ok( GgaMessage { 
            utc_time: UtcTime::try_from(sections.nth(1).unwrap())                               .unwrap(),//.map_err(GgaParseError::UtcParseError)?, 
            latitude: Degrees::try_from((sections.nth(2).unwrap(), sections.nth(3).unwrap()))   .unwrap(),//.map_err(GgaParseError::LatLongParseError)?, 
            longitude: Degrees::try_from((sections.nth(4).unwrap(), sections.nth(5).unwrap()))  .unwrap(),//.map_err(GgaParseError::LatLongParseError)?, 
            fix_type: GpsFixType::try_from(sections.nth(6).unwrap())                            .unwrap(),//.map_err(|_| GgaParseError::InvalidGpsFixType)?, 
            num_satellites: sections.nth(7).unwrap().parse()                                    .unwrap(),//.map_err(GgaParseError::InvalidSatelliteNumber)?, 
            altitude_msl: Altitude::try_from(sections.nth(9).unwrap())                          .unwrap(),//.map_err(GgaParseError::AltitudeParseError)?, 
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

pub struct Altitude(u32); // Stored as cm internally. Good up to 65km high.
impl Altitude {
    pub fn metres(&self) -> u16 {
        (self.0 / 100) as u16
    }
    pub fn centimetres(&self) -> u32 {
        self.0
    }
}
impl TryFrom<&str> for Altitude {
    type Error = ParseIntError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        crate::println!("{}", value);
        match value.split_once('.') {
            Some((metres, frac)) => {
                Ok(Altitude(metres.parse::<u32>()? * 100 + frac[..2].parse::<u32>()?)) // Fractional part at most 2dp.
            },
            None => {
                Ok(Altitude(value.parse::<u32>()? * 100))
            },
        }
    }
}
impl uDisplay for Altitude {
    fn fmt<W>(&self, f: &mut ufmt::Formatter<'_, W>) -> Result<(), W::Error>
    where
        W: ufmt::uWrite + ?Sized { 
        let frac = self.centimetres() % 100;
        if frac < 10 {  uwrite!(f, "{}.0{}m", self.metres(), frac) }
        else {          uwrite!(f, "{}.{}m",  self.metres(), frac) }
        
    }
}