extern crate mmap;
extern crate rustc_serialize;

// TODO: Either generalize this code to Seek+Read, or extend MemoryMap with
// those traits.
// But why?

#[macro_use]
mod util;
mod patch;
mod revlog;

use std::{error, result};
use rustc_serialize::hex::ToHex;

fn read_revlog(path: &str) -> result::Result<(), Box<error::Error>> {
    let revlog = try!(revlog::Revlog::open(path));

    println!("   rev    offset  length  {} linkrev nodeid       p1           p2",
             if revlog.generaldelta {
                 "delta"
             } else {
                 " base"
             });

    for entry in revlog.iter() {
        let entry = try!(entry);

        let p1 = entry.parent_1_id().map(|s| s.to_hex()).unwrap();
        let p2 = entry.parent_2_id().map(|s| s.to_hex()).unwrap();
        println!("{:6} {:9} {:7} {:6} {:7} {} {} {}",
                 entry.revno,
                 entry.offset(),
                 entry.chunk.comp_len(),
                 entry.base_rev(),
                 entry.chunk.link_rev(),
                 &entry.chunk.c_node_id()[..6].to_hex(),
                 &p1[..12],
                 &p2[..12]);

        let mut chain: Vec<_>;
        chain = entry.delta_chain().map(Result::unwrap).collect();
        chain.reverse();
        let base = chain.pop().unwrap();
        let patches: Vec<Vec<u8>>;
        patches = chain.iter().map(|rev| rev.data()).collect();
        patch::apply(base.data(), patches);
    }
    Ok(())
}


fn main() {
    for path in std::env::args().skip(1) {
        match read_revlog(&path) {
            Ok(()) => (),
            Err(e) => println!("Err({:?})", e),
        }
    }
}
