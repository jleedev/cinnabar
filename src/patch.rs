//! A patch operation consists of a base revision and a sequence of
//! patches. The buffer is initialized with the contents of the base
//! revision and patches are applied in sequence.
//!
//! Each patch is a sequence of length-delimited hunks.
//! Each hunk contains three u32le values followed by `c` data bytes.
//!
//! `a` - seek to this position in the buffer  
//! `b` - delete to this position in the buffer (`b` &ge; `a`)  
//! `c` - length of the value to insert
//!
//! This implementation is as naive as possible: it actually copies and
//! moves all the bytes around as needed. Areas for improvement: Use a
//! data structure that can perform this lazily.

extern crate byteorder;

use patch::byteorder::{BigEndian,ReadBytesExt};
use std::io::{Cursor,Read};

pub fn apply(base: Vec<u8>, patches: Vec<Vec<u8>>) -> Vec<u8> {
    let mut buf = Cursor::new(base);
    for patch in patches {
        let len = patch.len() as u64;
        let mut cur = Cursor::new(patch);
        while cur.position() != len {
            let (a, b, c) = decode_header(&mut cur);
            println!("{} {} {}", a, b, c);
            let mut x = Vec::with_capacity(c as usize);
            cur.read_exact(&mut x[..]).unwrap();
        }
    };
    return vec![];
}

fn decode_header(header: &mut Cursor<Vec<u8>>) -> (u32, u32, u32) {
    let a = header.read_u32::<BigEndian>().unwrap();
    let b = header.read_u32::<BigEndian>().unwrap();
    let c = header.read_u32::<BigEndian>().unwrap();
    return (a, b, c);
}

#[cfg(test)]
mod test {
    use super::decode_header;
    use std::io::Cursor;
    #[test]
    fn test_header() {
        let mut hdr = Cursor::new(
            b"\x00\x00\x00\x2a\x00\x00\x00\x2b\x00\x00\x00\x2c" as &[u8]);
        assert_eq!((0x2a, 0x2b, 0x2c), decode_header(&mut hdr));
    }
}
