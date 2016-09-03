extern crate core;
extern crate mmap;
extern crate rustc_serialize;

// TODO: Either generalize this code to Seek+Read, or extend MemoryMap with
// those traits.
// But why?

#[macro_use]
mod util;
mod revlog;

use std::{error, result};
use rustc_serialize::hex::ToHex;

fn dump_revlog_hex(data: &[u8]) {
    if data.len() == 0 {
        return;
    }
    let (x, xs) = data.split_at(16);
    println!("{}", x.to_hex());
    dump_revlog_hex(xs);
}

pub fn read_revlog(path: &str) -> result::Result<(), Box<error::Error>> {
    let revlog = try!(revlog::Revlog::open(path));
    for (i, entry) in revlog.iter().enumerate() {
        println!("{} => {}", i, try!(entry));
    }
    // for i in 0..10 {
    // println!("{} => {}", i, try!(revlog.entry(i)));
    // }
    //
    Ok(())
}


fn main() {
    for path in std::env::args().skip(1) {
        match read_revlog(&path) {
            Ok(()) => (),
            Err(e) => println!("{}", e),
        }
    }
}
