

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

type ShFlag = Option<std::sync::Arc<std::sync::atomic::AtomicBool>>;

/// Read and ignore all the data in a separate thread
fn drain(mut s: impl std::io::Read + Send + 'static, close_notification: ShFlag) {
    std::thread::spawn(move|| {
        let mut buf = [0u8; 1024];
        loop {
            match s.read(&mut buf  [0..]) {
                Ok(0) => break,
                Ok(_x) => (),
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    sleep(10);
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => (),
                Err(_e) => break,
            }
        }
        if let Some(ref x) = close_notification {
            x.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    });
}


fn sleep(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

// Check if docket gets disconnected 
fn check_closedness(s: &TcpStream, how_long_to_wait: Duration, additional_close_notification: ShFlag) -> Result<bool> {
    let deadline = Instant::now() + how_long_to_wait;
    loop {
        if Instant::now() > deadline {
            return Ok(false)
        }

        if let Some(ref acn) = additional_close_notification {
            if acn.load(std::sync::atomic::Ordering::SeqCst) {
                return Ok(true);
            }
        }

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


#[derive(Clone,Copy, Debug)]
enum CloseDetectMode {
    CloseIncomingCheckOutgoing,
    CloseOutgoingCheckIncoming,
}

#[derive(Clone,Copy,Debug)]
enum WritingPolicy {
    Ignore,
    Shutdown,
    Clog,
}
#[derive(Clone,Copy,Debug)]
enum ReadingPolicy {
    Ignore,
    Drain,
}

#[derive(Clone,Copy,Debug)]
struct CloseDetectOpts {
    report_buffer_sizes : bool,
    outgoing_write: WritingPolicy,
    outgoing_read: ReadingPolicy,
    incoming_write: WritingPolicy,
    incoming_read: ReadingPolicy,
    mode: CloseDetectMode,
}

impl std::fmt::Display for CloseDetectOpts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "out")?;
        match self.outgoing_write {
            WritingPolicy::Ignore => (),
            WritingPolicy::Shutdown => write!(f, "Shut")?,
            WritingPolicy::Clog => write!(f, "Clog")?,
        }
        match self.outgoing_read {
            ReadingPolicy::Ignore => (),
            ReadingPolicy::Drain => write!(f, "Drain")?,
        }
        match self.mode {
            CloseDetectMode::CloseIncomingCheckOutgoing => write!(f, "Check")?,
            CloseDetectMode::CloseOutgoingCheckIncoming => write!(f, "Close")?,
        }

        write!(f, "_in")?;
        match self.incoming_write {
            WritingPolicy::Ignore => (),
            WritingPolicy::Shutdown => write!(f, "Shut")?,
            WritingPolicy::Clog => write!(f, "Clog")?,
        }
        match self.incoming_read {
            ReadingPolicy::Ignore => (),
            ReadingPolicy::Drain => write!(f, "Drain")?,
        }
        match self.mode {
            CloseDetectMode::CloseIncomingCheckOutgoing => write!(f, "Close")?,
            CloseDetectMode::CloseOutgoingCheckIncoming => write!(f, "Check")?,
        }
        Ok(())
    }
}

/// Clog both directions of the tunnel by writing, but not reading the data.
/// Then close of of the sockets. Will RST propagate to the other end?  
fn closedetect(opts: &Opts, cdo: CloseDetectOpts) -> Result<()> {
    match (cdo.incoming_read, cdo.outgoing_write) {
        (ReadingPolicy::Drain, WritingPolicy::Clog) => return Ok(()),
        _ => (),
    }
    match (cdo.outgoing_read, cdo.incoming_write) {
        (ReadingPolicy::Drain, WritingPolicy::Clog) => return Ok(()),
        _ => (),
    }
    match cdo.mode {
        CloseDetectMode::CloseIncomingCheckOutgoing => {
            match (cdo.outgoing_write, cdo.outgoing_read) {
                (WritingPolicy::Clog, _) => (),
                (_, ReadingPolicy::Drain) => (),
                _ => return Ok(()),
            }
        }
        CloseDetectMode::CloseOutgoingCheckIncoming => {
            match (cdo.incoming_write, cdo.incoming_read) {
                (WritingPolicy::Clog, _) => (),
                (_, ReadingPolicy::Drain) => (),
                _ => return Ok(()),
            }
        }
    }

    let ss = TcpListener::bind(opts.listen)?;
    let g  = std::thread::spawn(move || -> Result<(TcpStream, ShFlag)> {
        let mut cn : ShFlag = None;
        let mut cc = ss.accept()?.0;
        drop(ss);
        cc.set_nonblocking(true)?;
        match cdo.incoming_read {
            ReadingPolicy::Ignore => (),
            ReadingPolicy::Drain => {
                cn = Some(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)));
                drain(cc.try_clone()?, cn.clone());
            }
        }
        match cdo.incoming_write {
            WritingPolicy::Ignore => (),
            WritingPolicy::Shutdown => {
                cc.shutdown(std::net::Shutdown::Write)?
            }
            WritingPolicy::Clog => {
                let sz = clog(&mut cc)?;
                if cdo.report_buffer_sizes {
                    println!("One direction buffer: {}", sz);
                }
            }
        }

        //sleep(5000);
        Ok((cc, cn))
    });
    let mut cs = TcpStream::connect(opts.connect)?;
    let mut cs_close : ShFlag = None;
    cs.set_nonblocking(true)?;


    match cdo.outgoing_read {
        ReadingPolicy::Ignore => (),
        ReadingPolicy::Drain => {
            cs_close = Some(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)));
            drain(cs.try_clone()?, cs_close.clone());
        }
    }
    match cdo.outgoing_write {
        WritingPolicy::Ignore => (),
        WritingPolicy::Shutdown => {
            cs.shutdown(std::net::Shutdown::Write)?;
        }
        WritingPolicy::Clog => {
            let sz = clog(&mut cs)?;
            if cdo.report_buffer_sizes {
                println!("The other direction buffer: {}", sz);
            }
        }
    }


    let (cc,cc_close) = g.join().unwrap()?;

    // Now both `cs` and `cc` sockets are fully clogged. Let's close one of them and see what happens to the other one.

    sleep(100);

    let (s, closenotif) = match cdo.mode {
        CloseDetectMode::CloseIncomingCheckOutgoing => {
            drop(cc);
            (cs, cs_close)
        }
        CloseDetectMode::CloseOutgoingCheckIncoming => {
            drop(cs);
            (cc, cc_close)

        }
    };

    if check_closedness(&s, Duration::from_millis(500), closenotif)? {
        println!("[ OK ] Clogged close test passed: {}", cdo);
    } else {
        println!("[FAIL] Clogged close test failed: {}", cdo);
    }

    Ok(())
}

fn main() -> Result<()> {
    let opts : Opts = argh::from_env(); 

    trivial_test_1(&opts)?;

    trivial_test_2(&opts)?;

    let mut report_buffer_sizes = true;

    let mut cd_battery1 = |mode:CloseDetectMode|  {
        let mut cd_battery2 = |incoming_write: WritingPolicy| {
            let mut cd_battery3 = |outgoing_write: WritingPolicy| {
                let mut cd_battery4 = |incoming_read:ReadingPolicy| {
                    let mut cd_battery5 = |outgoing_read: ReadingPolicy| {
                        let cdo = CloseDetectOpts {
                            report_buffer_sizes,
                            incoming_write,
                            outgoing_write,
                            incoming_read,
                            outgoing_read,
                            mode,
                        };
                        if let Err(e) = closedetect(&opts, cdo) {
                            eprintln!("{}", e);
                        }
                        report_buffer_sizes = false;
                    };
                    cd_battery5(ReadingPolicy::Ignore);
                    cd_battery5(ReadingPolicy::Drain);
                };
                cd_battery4(ReadingPolicy::Ignore);
                cd_battery4(ReadingPolicy::Drain);
            };
            cd_battery3(WritingPolicy::Ignore);
            cd_battery3(WritingPolicy::Clog);
            cd_battery3(WritingPolicy::Shutdown);
        };
        cd_battery2(WritingPolicy::Ignore);
        cd_battery2(WritingPolicy::Clog);
        cd_battery2(WritingPolicy::Shutdown);
    };

    cd_battery1(CloseDetectMode::CloseIncomingCheckOutgoing);
    cd_battery1(CloseDetectMode::CloseOutgoingCheckIncoming);

    Ok(())
}
