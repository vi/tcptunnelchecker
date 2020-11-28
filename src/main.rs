#![allow(unused)]

use std::net::SocketAddr;
use std::net::{TcpListener,TcpStream};
use anyhow::Result;
use std::time::{Duration,Instant};
use std::io::ErrorKind;

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
    println!("[ OK ] Trivial test 1");
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
    println!("[ OK ] Trivial test 2");
    Ok(())
}

/// Write as much data as possible to this nonblocking writer
fn clog(mut s: impl std::io::Write) -> Result<usize> {
    let buf = [0u8; 1024];
    let mut writelen = 1024;
    let mut waitctr = 6;
    let mut written : usize = 0;
    loop {
        match s.write(&buf[0..writelen]) {
            Ok(0) => anyhow::bail!("Write to socket returned 0?"),
            Ok(x) => written += x,
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                if waitctr > 0 {
                    sleep(25);
                    waitctr-=1;
                    continue;
                } else if writelen > 1 {
                    writelen = 1;
                    continue;
                }
                return Ok(written);
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => Err(e)?,
        }
    }
}
/// Read and ignore all the data in a separate thread
fn drain(mut s: impl std::io::Read + Send + 'static) {
    std::thread::spawn(move|| {
        let mut buf = [0u8; 1024];
        loop {
            match s.read(&mut buf  [0..]) {
                Ok(0) => return,
                Ok(x) => (),
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    sleep(10);
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => (),
                Err(e) => return,
            }
        }
    });
}


fn sleep(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

// Check if docket gets disconnected 
fn check_closedness(mut s: &TcpStream, how_long_to_wait: Duration) -> Result<bool> {
    let deadline = Instant::now() + how_long_to_wait;
    loop {
        if Instant::now() > deadline {
            return Ok(false)
        }

        use std::io::Write;
        //let buf=[0u8; 1];
        match s.take_error() {
            Ok(None) => (),
            Ok(Some(e)) if e.kind() == ErrorKind::ConnectionAborted => return Ok(true),
            Ok(Some(e)) if e.kind() == ErrorKind::ConnectionReset => return Ok(true),
            Ok(Some(e)) if e.kind() == ErrorKind::BrokenPipe => return Ok(true),
            Ok(Some(e)) => Err(e)?,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) if e.kind() == ErrorKind::WouldBlock => (),
            Err(e) => Err(e)?,
        }

        sleep(50);
    }
}

#[derive(Clone,Copy)]
struct CloseDetectOpts {
    report_buffer_sizes : bool,
    clog_incoming: bool,
    clog_outgoing: bool,
    check_incoming_for_closedness: bool,
    shutdown_incoming_for_writing: bool,
    shutdown_outgoing_for_writing: bool,
    drain_incoming: bool,
    drain_outgoing: bool,
    experiment_name: &'static str,
}

/// Clog both directions of the tunnel by writing, but not reading the data.
/// Then close of of the sockets. Will RST propagate to the other end?  
fn closedetect(opts: &Opts, cdo: CloseDetectOpts) -> Result<()> {
    use std::io::{Read,Write};
    let ss = TcpListener::bind(opts.listen)?;
    let g  = std::thread::spawn(move || -> Result<TcpStream> {
        let mut cc = ss.accept()?.0;
        drop(ss);
        cc.set_nonblocking(true)?;
        if cdo.shutdown_incoming_for_writing {
            cc.shutdown(std::net::Shutdown::Write)?;
        }
        if cdo.drain_incoming {
            drain(cc.try_clone()?);
        }
        if cdo.clog_incoming {
            let sz = clog(&mut cc)?;
            if cdo.report_buffer_sizes {
                println!("One direction buffer: {}", sz);
            }
        }
        //sleep(5000);
        Ok(cc)
    });
    let mut cs = TcpStream::connect(opts.connect)?;
    cs.set_nonblocking(true)?;
    if cdo.shutdown_outgoing_for_writing {
        cs.shutdown(std::net::Shutdown::Write)?;
    }
    
    if cdo.drain_outgoing {
        drain(cs.try_clone()?);
    }
    if cdo.clog_outgoing {
        let sz = clog(&mut cs)?;
        if cdo.report_buffer_sizes {
            println!("The other direction buffer: {}", sz);
        }
    }

    let cc = g.join().unwrap()?;

    // Now both `cs` and `cc` sockets are fully clogged. Let's close one of them and see what happens to the other one.

    sleep(100);

    let s = if cdo.check_incoming_for_closedness {
        drop(cs);
        cc
    } else {
        drop(cc);
        cs
    };


    if check_closedness(&s, Duration::from_millis(500))? {
        println!("[ OK ] Clogged close test {} passed", cdo.experiment_name);
    } else {
        println!("[FAIL] Clogged close test {} failed!", cdo.experiment_name);
    }

    Ok(())
}

fn main() -> Result<()> {
    let opts : Opts = argh::from_env(); 

    trivial_test_1(&opts)?;
    trivial_test_2(&opts)?;

    let cdo = CloseDetectOpts {
        report_buffer_sizes : true,
        clog_incoming : true,
        clog_outgoing : true,
        check_incoming_for_closedness: true,
        shutdown_incoming_for_writing: false,
        shutdown_outgoing_for_writing: false,
        drain_incoming: false,
        drain_outgoing: false,
        experiment_name: "1",
    };
    closedetect(&opts, cdo)?;

    let cdo = CloseDetectOpts {
        report_buffer_sizes : false,
        clog_incoming : true,
        clog_outgoing : true,
        check_incoming_for_closedness: false,
        shutdown_incoming_for_writing: false,
        shutdown_outgoing_for_writing: false,
        drain_incoming: false,
        drain_outgoing: false,
        experiment_name: "2",
    };
    closedetect(&opts, cdo)?;

    let cdo = CloseDetectOpts {
        report_buffer_sizes : false,
        clog_incoming : false,
        clog_outgoing : true,
        check_incoming_for_closedness: false,
        shutdown_incoming_for_writing: false,
        shutdown_outgoing_for_writing: false,
        drain_incoming: false,
        drain_outgoing: false,
        experiment_name: "3",
    };
    closedetect(&opts, cdo)?;

    let cdo = CloseDetectOpts {
        report_buffer_sizes : false,
        clog_incoming : true,
        clog_outgoing : false,
        check_incoming_for_closedness: true,
        shutdown_incoming_for_writing: false,
        shutdown_outgoing_for_writing: false,
        drain_incoming: false,
        drain_outgoing: false,
        experiment_name: "4",
    };
    closedetect(&opts, cdo)?;


    let cdo = CloseDetectOpts {
        report_buffer_sizes : false,
        clog_incoming : false,
        clog_outgoing : true,
        check_incoming_for_closedness: false,
        shutdown_incoming_for_writing: true,
        shutdown_outgoing_for_writing: false,
        drain_incoming: false,
        drain_outgoing: false,
        experiment_name: "5",
    };
    closedetect(&opts, cdo)?;


    let cdo = CloseDetectOpts {
        report_buffer_sizes : false,
        clog_incoming : true,
        clog_outgoing : false,
        check_incoming_for_closedness: true,
        shutdown_incoming_for_writing: false,
        shutdown_outgoing_for_writing: true,
        drain_incoming: false,
        drain_outgoing: false,
        experiment_name: "6",
    };
    closedetect(&opts, cdo)?;


    let cdo = CloseDetectOpts {
        report_buffer_sizes : false,
        clog_incoming : false,
        clog_outgoing : true,
        check_incoming_for_closedness: false,
        shutdown_incoming_for_writing: false,
        shutdown_outgoing_for_writing: false,
        drain_incoming: false,
        drain_outgoing: true,
        experiment_name: "7",
    };
    closedetect(&opts, cdo)?;

    let cdo = CloseDetectOpts {
        report_buffer_sizes : false,
        clog_incoming : true,
        clog_outgoing : false,
        check_incoming_for_closedness: true,
        shutdown_incoming_for_writing: false,
        shutdown_outgoing_for_writing: false,
        drain_incoming: true,
        drain_outgoing: false,
        experiment_name: "8",
    };
    closedetect(&opts, cdo)?;


    Ok(())
}
