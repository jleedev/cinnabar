extern crate mmap;

use mmap::{MapOption,MemoryMap};
use std::os::unix::io::AsRawFd;
use std::fs;
use std::fmt;

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

impl fmt::Display for RevlogEntry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
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

fn read_revlog_entry<'a> (m: &'a MemoryMap, i: isize) -> &'a RevlogEntry {
    unsafe {
        let p = m.data().offset(i) as *const [u8; 64];
        dump_revlog_hex(&*p);
        &*(p as *const RevlogEntry)
    }
}

fn read_revlog(path: &str) {
    let attr = fs::metadata(path).unwrap();
    assert!(attr.is_file(), "{} isn't a file", path);
    let f = fs::OpenOptions::new()
        .open(path).unwrap();
    let m = MemoryMap::new(attr.len() as usize, &[
        MapOption::MapReadable,
        MapOption::MapFd(f.as_raw_fd())]).unwrap();
    println!("{:?}", read_revlog_entry(&m, 0));
    return;
}

fn main() {
    for path in std::env::args().skip(1) {
        read_revlog(&path);
    }
}
