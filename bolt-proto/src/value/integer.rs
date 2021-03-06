use std::convert::{TryFrom, TryInto};
use std::mem;
use std::panic::catch_unwind;
use std::sync::{Arc, Mutex};

use bytes::{Buf, BufMut, Bytes, BytesMut};

use crate::error::*;
use crate::serialization::*;

mod conversions;

pub(crate) const MARKER_INT_8: u8 = 0xC8;
pub(crate) const MARKER_INT_16: u8 = 0xC9;
pub(crate) const MARKER_INT_32: u8 = 0xCA;
pub(crate) const MARKER_INT_64: u8 = 0xCB;

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct Integer {
    // Since integers come in many sizes, just store the bytes directly
    pub(crate) bytes: BytesMut,
}

impl Marker for Integer {
    fn get_marker(&self) -> Result<u8> {
        let value = match self.bytes.len() {
            1 => self.bytes.clone().get_i8() as i64,
            2 => self.bytes.clone().get_i16() as i64,
            4 => self.bytes.clone().get_i32() as i64,
            8 => self.bytes.clone().get_i64() as i64,
            _ => return Err(Error::ValueTooLarge(self.bytes.len())),
        };
        match value {
            -9_223_372_036_854_775_808..=-2_147_483_649
            | 2_147_483_648..=9_223_372_036_854_775_807 => Ok(MARKER_INT_64),
            -2_147_483_648..=-32_769 | 32_768..=2_147_483_647 => Ok(MARKER_INT_32),
            -32_768..=-129 | 128..=32_767 => Ok(MARKER_INT_16),
            -128..=-17 => Ok(MARKER_INT_8),
            -16..=127 => Ok(value as u8),
        }
    }
}

impl Serialize for Integer {}

impl TryInto<Bytes> for Integer {
    type Error = Error;

    fn try_into(mut self) -> Result<Bytes> {
        let mut bytes = BytesMut::with_capacity(mem::size_of::<u8>() + self.bytes.len());
        let marker = self.get_marker()?;
        bytes.put_u8(marker);
        // We first put our bytes in little-endian order so we only store as many bytes as we
        // actually need
        self.bytes.reverse();
        match marker {
            MARKER_INT_8 => bytes.put_i8(self.bytes.get_i8()),
            MARKER_INT_16 => bytes.put_i16(self.bytes.get_i16_le()),
            MARKER_INT_32 => bytes.put_i32(self.bytes.get_i32_le()),
            MARKER_INT_64 => bytes.put_i64(self.bytes.get_i64_le()),
            _ => {} // tiny int, value is already in the marker
        }
        Ok(bytes.freeze())
    }
}

impl Deserialize for Integer {}

impl TryFrom<Arc<Mutex<Bytes>>> for Integer {
    type Error = Error;

    fn try_from(input_arc: Arc<Mutex<Bytes>>) -> Result<Self> {
        catch_unwind(move || {
            let mut input_bytes = input_arc.lock().unwrap();
            let marker = input_bytes.get_u8();

            match marker {
                marker if (-16..=127).contains(&(marker as i8)) => Ok(Integer::from(marker as i8)),
                MARKER_INT_8 => Ok(Integer::from(input_bytes.get_i8())),
                MARKER_INT_16 => Ok(Integer::from(input_bytes.get_i16())),
                MARKER_INT_32 => Ok(Integer::from(input_bytes.get_i32())),
                MARKER_INT_64 => Ok(Integer::from(input_bytes.get_i64())),
                _ => Err(DeserializationError::InvalidMarkerByte(marker).into()),
            }
        })
        .map_err(|_| DeserializationError::Panicked)?
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;

    use super::*;

    #[test]
    fn get_marker() {
        let tiny = Integer::from(-16_i8);
        assert_eq!(tiny.get_marker().unwrap(), 0xF0);
        let small = Integer::from(-50_i8);
        assert_eq!(small.get_marker().unwrap(), MARKER_INT_8);
        let medium = Integer::from(-8000_i16);
        assert_eq!(medium.get_marker().unwrap(), MARKER_INT_16);
        let large = Integer::from(-1_000_000_000_i32);
        assert_eq!(large.get_marker().unwrap(), MARKER_INT_32);
        let very_large = Integer::from(-9_000_000_000_000_000_000_i64);
        assert_eq!(very_large.get_marker().unwrap(), MARKER_INT_64);
    }

    #[test]
    fn try_into_bytes() {
        let tiny = Integer::from(-16_i8);
        assert_eq!(tiny.try_into_bytes().unwrap(), Bytes::from_static(&[0xF0]));
        let tiny_64 = Integer::from(5_i64);
        assert_eq!(
            tiny_64.try_into_bytes().unwrap(),
            Bytes::from_static(&[0x05])
        );
        let small = Integer::from(-50_i8);
        assert_eq!(
            small.try_into_bytes().unwrap(),
            Bytes::from_static(&[MARKER_INT_8, 0xCE])
        );
        let small_64 = Integer::from(-50_i64);
        assert_eq!(
            small_64.try_into_bytes().unwrap(),
            Bytes::from_static(&[MARKER_INT_8, 0xCE])
        );
        let medium = Integer::from(-8000_i16);
        assert_eq!(
            medium.try_into_bytes().unwrap(),
            Bytes::from_static(&[MARKER_INT_16, 0xE0, 0xC0])
        );
        let large = Integer::from(-1_000_000_000_i32);
        assert_eq!(
            large.try_into_bytes().unwrap(),
            Bytes::from_static(&[MARKER_INT_32, 0xC4, 0x65, 0x36, 0x00])
        );
        let very_large = Integer::from(-9_000_000_000_000_000_000_i64);
        assert_eq!(
            very_large.try_into_bytes().unwrap(),
            Bytes::from_static(&[
                MARKER_INT_64,
                0x83,
                0x19,
                0x93,
                0xAF,
                0x1D,
                0x7C,
                0x00,
                0x00
            ])
        );
    }

    #[test]
    fn try_from_bytes() {
        let tiny = Integer::from(-16_i8);
        assert_eq!(
            Integer::try_from(Arc::new(Mutex::new(tiny.clone().try_into_bytes().unwrap())))
                .unwrap(),
            tiny
        );
        let small = Integer::from(-50_i8);
        assert_eq!(
            Integer::try_from(Arc::new(Mutex::new(
                small.clone().try_into_bytes().unwrap()
            )))
            .unwrap(),
            small
        );
        let medium = Integer::from(-8000_i16);
        assert_eq!(
            Integer::try_from(Arc::new(Mutex::new(
                medium.clone().try_into_bytes().unwrap()
            )))
            .unwrap(),
            medium
        );
        let large = Integer::from(-1_000_000_000_i32);
        assert_eq!(
            Integer::try_from(Arc::new(Mutex::new(
                large.clone().try_into_bytes().unwrap()
            )))
            .unwrap(),
            large
        );
        let very_large = Integer::from(-9_000_000_000_000_000_000_i64);
        assert_eq!(
            Integer::try_from(Arc::new(Mutex::new(
                very_large.clone().try_into_bytes().unwrap()
            )))
            .unwrap(),
            very_large
        );
    }

    #[test]
    fn zero_extension() {
        let number = 12123_i16;
        let number_bytes = &[0xC9, 0x2F, 0x5B];
        assert_eq!(
            Integer::from(number).try_into_bytes().unwrap().as_ref(),
            number_bytes,
        );
        assert_eq!(
            i32::from(
                Integer::try_from(Arc::new(Mutex::new(Bytes::from_static(number_bytes)))).unwrap()
            ),
            number as i32
        );
        assert_eq!(
            i64::from(
                Integer::try_from(Arc::new(Mutex::new(Bytes::from_static(number_bytes)))).unwrap()
            ),
            number as i64
        );
    }

    #[test]
    fn sign_extension() {
        let number = -60_i8;
        let number_bytes = &[0xC8, 0xC4];
        assert_eq!(
            Integer::from(number).try_into_bytes().unwrap().as_ref(),
            number_bytes,
        );
        assert_eq!(
            i32::from(
                Integer::try_from(Arc::new(Mutex::new(Bytes::from_static(number_bytes)))).unwrap()
            ),
            number as i32
        );
        assert_eq!(
            i64::from(
                Integer::try_from(Arc::new(Mutex::new(Bytes::from_static(number_bytes)))).unwrap()
            ),
            number as i64
        );
    }
}
