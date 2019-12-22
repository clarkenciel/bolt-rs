use std::convert::TryFrom;
use std::mem;
use std::panic::catch_unwind;

use bytes::{Buf, BufMut, Bytes, BytesMut};
use failure::Error;
use tokio::io::BufStream;
use tokio::net::TcpStream;
use tokio::prelude::*;

use crate::error::DeserializeError;
use crate::message::Chunk;

#[derive(Debug)]
pub struct MessageBytes {
    pub(crate) bytes: BytesMut,
}

impl MessageBytes {
    pub fn new() -> MessageBytes {
        MessageBytes {
            bytes: BytesMut::new(),
        }
    }

    pub fn with_capacity(capacity: usize) -> MessageBytes {
        MessageBytes {
            bytes: BytesMut::with_capacity(capacity),
        }
    }

    pub fn add_chunk(&mut self, chunk: Chunk) {
        self.bytes.put(chunk.data);
    }

    pub async fn from_stream(buf_stream: &mut BufStream<TcpStream>) -> Result<MessageBytes, Error> {
        let mut message = MessageBytes::new();
        loop {
            let size = buf_stream.read_u16().await? as usize;
            if size == 0 {
                // We've reached the end of the message
                // Note that after this point we will have consumed the last two 0 bytes
                break;
            }
            let mut buf = BytesMut::with_capacity(size);
            buf_stream.read_buf(&mut buf).await?;
            debug_assert!(buf.len() == size);
            message.add_chunk(Chunk::try_from(buf.freeze())?)
        }
        Ok(message)
    }
}

impl TryFrom<Bytes> for MessageBytes {
    type Error = Error;

    fn try_from(mut bytes: Bytes) -> Result<MessageBytes, Self::Error> {
        let result: Result<MessageBytes, Error> = catch_unwind(move || {
            let mut message = MessageBytes::with_capacity(bytes.len());
            while bytes.has_remaining() {
                let size: u16 = bytes.get_u16();
                if size == 0 && !bytes.has_remaining() {
                    // We've reached the end of the message
                    break;
                }
                let mut buf = BytesMut::with_capacity(size as usize);
                for _ in 0..size {
                    buf.put_u8(bytes.get_u8());
                }
                debug_assert!(buf.len() == size as usize);
                message.add_chunk(Chunk::try_from(buf.freeze())?)
            }
            Ok(message)
        })
        .map_err(|_| DeserializeError("Panicked during deserialization".to_string()))?;

        Ok(result.map_err(|err: Error| {
            DeserializeError(format!("Error creating Message from Bytes: {}", err))
        })?)
    }
}

impl Into<Bytes> for MessageBytes {
    // TODO: This puts the message into a single chunk, consider breaking up large messages into several chunk
    fn into(self) -> Bytes {
        let mut bytes = BytesMut::with_capacity(
            mem::size_of::<u8>() * 2 + self.bytes.len() + mem::size_of::<u8>() * 2,
        );
        bytes.put_u16(self.bytes.len() as u16);
        bytes.put(self.bytes);
        bytes.put_u16(0);
        bytes.freeze()
    }
}

#[cfg(test)]
mod tests {
    use std::convert::TryFrom;

    use super::*;

    fn new_chunk() -> Chunk {
        Chunk::try_from(Bytes::from_static(&[
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D,
            0x0E, 0x0F,
        ]))
        .unwrap()
    }

    fn new_message() -> MessageBytes {
        let mut msg = MessageBytes::with_capacity(1);
        msg.add_chunk(new_chunk());
        msg
    }

    #[test]
    fn into_bytes() {
        let bytes: Bytes = new_message().into();
        let mut result = BytesMut::new();
        result.put_u16(new_chunk().data.len() as u16);
        result.put(new_chunk().data);
        result.put_u16(0);
        assert_eq!(bytes, result.freeze())
    }

    //    #[test]
    //    fn into_bytes_multiple_chunks() {
    //        todo!();
    //    }

    #[test]
    fn from_bytes() {
        let bytes = Bytes::from_static(&[
            0x00, 0x10, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B,
            0x0C, 0x0D, 0x0E, 0x0F, 0x00, 0x00,
        ]);
        let message = MessageBytes::try_from(bytes);
        assert_eq!(message.unwrap().bytes, new_chunk().data);
    }

    #[test]
    fn from_bytes_multiple_chunks() {
        let bytes = Bytes::from_static(&[
            0x00, 0x10, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B,
            0x0C, 0x0D, 0x0E, 0x0F, 0x00, 0x04, 0x01, 0x02, 0x03, 0x04, 0x00, 0x00,
        ]);
        let message = MessageBytes::try_from(bytes);
        assert_eq!(
            message.unwrap().bytes,
            Bytes::from_static(&[
                0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D,
                0x0E, 0x0F, 0x01, 0x02, 0x03, 0x04
            ])
        );
    }
}