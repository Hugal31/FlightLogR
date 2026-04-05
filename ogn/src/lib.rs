use core::{
    fmt::{Debug, Formatter},
    num::ParseIntError,
    str::FromStr,
};

pub mod ddb;

pub const OGN_APRS_URL: &'static str = "aprs.glidernet.org:14580";

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum AircraftType {
    Reserved,
    Glider,
    TowPlane,
    Helicopter,
    Skydiver,
    DropPlane,
    HandGlider,
    Paraglider,
    Aircraft,
    JetAircraft,
    Unknown,
    Balloon,
    Airship,
    Unmaned,
    Reserved2,
    StaticObstacle,
}

impl From<u8> for AircraftType {
    fn from(i: u8) -> Self {
        use AircraftType::*;
        match i {
            0 => Reserved,
            1 => Glider,
            2 => TowPlane,
            3 => Helicopter,
            4 => Skydiver,
            5 => DropPlane,
            6 => HandGlider,
            7 => Paraglider,
            8 => Aircraft,
            9 => JetAircraft,
            0xA => Unknown,
            0xB => Balloon,
            0xC => Airship,
            0xD => Unmaned,
            0xE => Reserved2,
            0xF => StaticObstacle,
            _ => Unknown,
        }
    }
}

impl Into<u8> for AircraftType {
    fn into(self) -> u8 {
        use AircraftType::*;
        match self {
            Reserved => 0,
            Glider => 1,
            TowPlane => 2,
            Helicopter => 3,
            Skydiver => 4,
            DropPlane => 5,
            HandGlider => 6,
            Paraglider => 7,
            Aircraft => 8,
            JetAircraft => 9,
            Unknown => 0xA,
            Balloon => 0xB,
            Airship => 0xC,
            Unmaned => 0xD,
            Reserved2 => 0xE,
            StaticObstacle => 0xF,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum AddressType {
    Unknown,
    ICAO,
    Flarm,
    OgnTracker,
}

impl From<u8> for AddressType {
    fn from(i: u8) -> Self {
        use AddressType::*;
        match i {
            1 => ICAO,
            2 => Flarm,
            3 => OgnTracker,
            _ => Unknown,
        }
    }
}

impl Into<u8> for AddressType {
    fn into(self) -> u8 {
        use AddressType::*;
        match self {
            Unknown => 0,
            ICAO => 1,
            Flarm => 2,
            OgnTracker => 3,
        }
    }
}

/// OGN-format beacon.
/// http://wiki.glidernet.org/wiki:ogn-flavoured-aprs
#[derive(Clone, Eq, PartialEq)]
pub struct Beacon {
    pub stealth: bool,
    pub no_tracking: bool,
    pub aircraft_type: AircraftType,
    pub address_type: AddressType,
    pub address: u32,
}

impl Beacon {
    fn get_address(beacon: u32) -> u32 {
        beacon & 0xFFFFFF
    }
}

impl Debug for Beacon {
    fn fmt(&self, fmt: &mut Formatter) -> core::fmt::Result {
        fmt.debug_struct("Beacon")
            .field("stealth", &self.stealth)
            .field("no_tracking", &self.no_tracking)
            .field("aircraft_type", &self.aircraft_type)
            .field("address_type", &self.address_type)
            .field("address", &format_args!("{:X}", self.address))
            .finish()
    }
}

impl FromStr for Beacon {
    type Err = ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let number = u32::from_str_radix(s, 16)?;

        let info = (number >> 24) as u8;
        let stealth = (info & 0b10000000) != 0;
        let no_tracking = (info & 0b01000000) != 0;
        let aircraft_type = ((info >> 2) & 0b00001111).into();
        let address_type = (info & 0b11).into();
        let address = Self::get_address(number);

        Ok(Beacon {
            stealth,
            no_tracking,
            aircraft_type,
            address_type,
            address,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse() {
        assert_eq!(
            "06DF0A52".parse::<Beacon>().expect("should parse"),
            Beacon {
                stealth: false,
                no_tracking: false,
                aircraft_type: AircraftType::Glider,
                address_type: AddressType::Flarm,
                address: 0xDF0A52,
            }
        );

        assert_eq!(
            "CD3E0F90".parse::<Beacon>().expect("should parse"),
            Beacon {
                stealth: true,
                no_tracking: true,
                aircraft_type: AircraftType::Helicopter,
                address_type: AddressType::ICAO,
                address: 0x3E0F90,
            }
        );
    }
}
