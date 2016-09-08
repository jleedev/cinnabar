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
//! This implementation uses an off-the-shelf rope structure to perform
//! this editing.

extern crate byteorder;
extern crate bytes;

use patch::byteorder::{BigEndian, ReadBytesExt};
use std::io::{Cursor, Read};
use self::bytes::{Bytes, Source};

pub fn apply(base: Vec<u8>, patches: Vec<Vec<u8>>) -> Vec<u8> {
    //println!("::: {:?}", String::from_utf8_lossy(&base));
    let mut buf: Bytes = From::from(&base);
    for patch in patches {
        let patch_len = patch.len() as u64;
        let mut cur = Cursor::new(patch);
        while cur.position() != patch_len {
            assert!(cur.position() < patch_len);
            let before_len = buf.len();
            let (a, b, c) = decode_header(&mut cur);
            let piece = read_slice(&mut cur, c);
            //println!("+++ {} {} {} {:?}", a, b, c, String::from_utf8_lossy(&piece));
            let insertion: Bytes = From::from(&piece);
            let left = buf.slice_to(a);
            let right = buf.slice_from(b);
            buf = left.concat(&insertion).concat(&right);
            assert_eq!(before_len - (b - a) + c, buf.len());
        }
    }
    let mut result = vec![];
    buf.copy_to(&mut result);
    return result;
}

fn read_slice(src: &mut Cursor<Vec<u8>>, len: usize) -> Vec<u8> {
    let mut buf = vec![0; len];
    src.read_exact(&mut buf[..]).unwrap();
    return buf;
}

fn decode_header(header: &mut Cursor<Vec<u8>>) -> (usize, usize, usize) {
    let a = header.read_u32::<BigEndian>().unwrap() as usize;
    let b = header.read_u32::<BigEndian>().unwrap() as usize;
    let c = header.read_u32::<BigEndian>().unwrap() as usize;
    return (a, b, c);
}

#[cfg(test)]
mod test {
    use super::decode_header;
    use std::io::Cursor;
    #[test]
    fn test_header() {
        let mut hdr = Cursor::new(b"\x00\x00\x00\x2a\x00\x00\x00\x2b\x00\x00\x00\x2c" as &[u8]);
        assert_eq!((0x2a, 0x2b, 0x2c), decode_header(&mut hdr));
    }
}
