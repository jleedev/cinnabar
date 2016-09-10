extern crate mmap;
extern crate rustc_serialize;
extern crate crypto;

// TODO: Either generalize this code to Seek+Read, or extend MemoryMap with
// those traits.
// But why?

#[macro_use]
mod util;
mod patch;
mod revlog;

use std::{error, result};
use rustc_serialize::hex::ToHex;
use crypto::sha1::Sha1;
use crypto::digest::Digest;

fn read_revlog(path: &str) -> result::Result<(), Box<error::Error>> {
    let revlog = try!(revlog::Revlog::open(path));

    println!("   rev    offset  length  {} linkrev nodeid       p1           p2",
             if revlog.generaldelta {
                 "delta"
             } else {
                 " base"
             });

    let mut good = 0;
    let mut bad = 0;
    for entry in revlog.iter() {
        let entry = try!(entry);

        let p1 = entry.parent_1_id().unwrap();
        let p2 = entry.parent_2_id().unwrap();
        let node_id = entry.chunk.c_node_id().to_hex();
        print_entry(&entry);
        //println!("{:?}", String::from_utf8_lossy(&entry.data()));

        let text = entry.text();
        // println!("{:?}", String::from_utf8_lossy(&text));

        let mut sha = Sha1::new();
        let mut ps = vec![p1, p2];
        ps.sort();
        for p in &ps {
            sha.input(p);
        }
        sha.input(&text);
        let hex = sha.result_str();
        if node_id != hex {
            bad += 1;
            println!("ERROR");
            println!("{:?}", String::from_utf8_lossy(&text));
        } else {
            good += 1;
            println!("verified {:?}", hex);
        }
        //assert_eq!(node_id, hex);
    }
    println!("{} hashes verified", good);
    println!("{} hashes failed", bad);
    Ok(())
}

fn print_entry(entry: &revlog::RevlogEntry) {
    let p1 = entry.parent_1_id().unwrap();
    let p2 = entry.parent_2_id().unwrap();
    let node_id = entry.chunk.c_node_id().to_hex();
    println!("{:6} {:9} {:7} {:6} {:7} {} {} {}",
             entry.revno,
             entry.offset(),
             entry.chunk.comp_len(),
             entry.base_rev(),
             entry.chunk.link_rev(),
             &node_id[..12],
             &p1.to_hex()[..12],
             &p2.to_hex()[..12]);
}


fn main() {
    for path in std::env::args().skip(1) {
        match read_revlog(&path) {
            Ok(()) => (),
            Err(e) => println!("Err({:?})", e),
        }
    }
}
