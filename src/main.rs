#![allow(unused)]

use std::net::SocketAddr;
use std::net::{TcpListener,TcpStream};
use anyhow::Result;

/// Check specified TCP tunnel (forwarder) implementation for defects and missing features
#[derive(argh::FromArgs)]
struct Opts {
    #[argh(positional)]
    listen: SocketAddr,

    #[argh(positional)]
    connect: SocketAddr,
}


fn trivial_test_1(opts: &Opts) -> Result<()> {
    use std::io::{Read,Write};
    let ss = TcpListener::bind(opts.listen)?;
    let g : std::thread::JoinHandle<Result<String>> = std::thread::spawn(move || -> Result<String> {
        let mut cc = ss.accept()?.0;
        drop(ss);
        let mut v = Vec::with_capacity(10);
        cc.read_to_end(&mut v)?;
        drop(cc);
        Ok(String::from_utf8(v)?)
    });
    let mut cs = TcpStream::connect(opts.connect)?;
    cs.write_all(b"Hello")?;
    drop(cs);
    let s = g.join().unwrap()?;
    anyhow::ensure!(s.len() > 0, "Empty string received from the forwarder");
    anyhow::ensure!(s == "Hello", "String mismatch after passing through the forwarder"); 
    println!("Trivial test 1 passed");
    Ok(())
}

fn trivial_test_2(opts: &Opts) -> Result<()> {
    use std::io::{Read,Write};
    let ss = TcpListener::bind(opts.listen)?;
    let g : std::thread::JoinHandle<Result<()>> = std::thread::spawn(move || -> Result<()> {
        let mut cc = ss.accept()?.0;
        drop(ss);
        cc.write_all(b"Hello")?;
        drop(cc);
        Ok(())
    });
    let mut cs = TcpStream::connect(opts.connect)?;
    let mut v = Vec::with_capacity(10);
    cs.read_to_end(&mut v)?;
    
    let s = String::from_utf8(v)?;

    drop(cs);
    let () = g.join().unwrap()?;

    anyhow::ensure!(s.len() > 0, "Empty string received from the forwarder");
    anyhow::ensure!(s == "Hello", "String mismatch after passing through the forwarder"); 
    println!("Trivial test 2 passed");
    Ok(())
}


fn main() -> Result<()> {
    let opts : Opts = argh::from_env(); 

    trivial_test_1(&opts)?;
    trivial_test_2(&opts)?;
    Ok(())
}
