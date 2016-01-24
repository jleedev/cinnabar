extern crate core;
extern crate mmap;

mod revlog {

use core::fmt::Write;
use mmap::{MapOption,MemoryMap};
use std::os::unix::io::AsRawFd;
use std::error;
use std::fs;
use std::fmt;
use std::result;

type Result<T> = result::Result<T, Box<error::Error>>;

/// A low-level cursor into RevlogNG index entry.
/// For instance, these fields do not yet take into account:
/// - Conversion from big endian
/// - Masking the version out of the first offset_flags
/// - Selecting the first 20 bytes of c_node_id
#[derive(Debug)]
struct RevlogChunk {
    offset_flags: u64,
    comp_len: u32,
    uncomp_len: u32,
    base_rev: u32,
    link_rev: u32,
    parent_1: u32,
    parent_2: u32,
    c_node_id: [u8; 32],
}

#[derive(Debug)]
struct RevlogEntry {
    /// Pointer to the data block
    chunk: RevlogChunk,
    /// Byte offset of chunk in the index file
    byte_offset: isize,
}

struct Revlog {
    path: String,
    mmap: MemoryMap,
}

impl Revlog {
    fn open(path: &str) -> Result<Revlog> {
        let attr = fs::metadata(path).unwrap();
        assert!(attr.is_file(), "{} isn't a file", path);
        let f = try!(fs::File::open(path));
        let m = try!(MemoryMap::new(attr.len() as usize, &[
            MapOption::MapReadable,
            MapOption::MapFd(f.as_raw_fd())]));
        return Ok(Revlog { path: String::from(path), mmap: m });
    }

    fn entry<'a> (&'a self, i: isize) -> &'a RevlogEntry {
        unsafe {
            let p = self.mmap.data().offset(i) as *const [u8; 64];
            dump_revlog_hex(&*p);
            &*(p as *const RevlogChunk)
        }
    }
}

impl fmt::Display for RevlogEntry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "comp_len: {}, uncomp_len: {}",
               u32::from_be(self.chunk.comp_len),
               u32::from_be(self.chunk.uncomp_len))
    }
}

fn dump_revlog_hex(data: &[u8; 64]) {
    for (i, b) in data.iter().enumerate() {
        if i > 0 && i % 4 == 0 { print!(" ") }
        if i > 0 && i % 16 == 0 { println!("") }
        print!("{:02x}", b);
    }
    println!("");
}

pub fn read_revlog(path: &str) {
    let revlog = Revlog::open(path).unwrap();
    println!("{}:", revlog.path);
    let entry = revlog.entry(0);
    println!("{:?}", entry);
    println!("");
}

}  // mod revlog

fn main() {
    for path in std::env::args().skip(1) {
        revlog::read_revlog(&path);
    }
}