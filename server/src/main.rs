#![allow(unused_imports)]
#![allow(unused_variables)]

use rand::Rng;
use std::io;
use std::io::{ Read, Write };
use std::mem;
use std::net::{ TcpListener, TcpStream };
use std::os::fd::{ AsRawFd, RawFd };
use std::sync::mpsc::{ channel, Receiver, Sender, TryRecvError };
use std::sync::{ Arc, Mutex };
use std::thread;
use std::time::Duration;
use std::time::Instant;

const ADDRESS: &str = "0.0.0.0:6666";
const NUM_THREADS: u8 = 8;
const MESSAGE_SIZE_MIN: usize = 128;
const MESSAGE_SIZE_MAX: usize = 2046;
const SLEEP_MS: u64 = 200;

enum Event {
  Client(TcpStream),
  Write(u16),
}

struct Client {
  tcp_stream: TcpStream,
  buffer: Vec<u8>,
}

fn set_linger(fd: RawFd, duration: Option<Duration>) -> io::Result<()> {
  let optval = libc::linger {
    l_onoff: duration.is_some().into(),
    l_linger: duration
      .map(|d| d.as_secs())
      .unwrap_or(0)
      .try_into()
      .expect("invalid linger duration"),
  };
  #[allow(clippy::undocumented_unsafe_blocks)]
  let result = unsafe {
    libc::setsockopt(
      fd,
      libc::SOL_SOCKET,
      libc::SO_LINGER,
      &optval as *const _ as *const libc::c_void,
      mem::size_of_val(&optval) as libc::socklen_t
    )
  };
  if result != 0 {
    return Err(io::Error::last_os_error());
  }
  Ok(())
}

fn set_so_value(
  fd: RawFd,
  optval: libc::c_int,
  level: libc::c_int,
  name: libc::c_int
) -> io::Result<()> {
  #[allow(clippy::undocumented_unsafe_blocks)]
  let result = unsafe {
    libc::setsockopt(
      fd,
      level,
      name,
      &optval as *const _ as *const libc::c_void,
      mem::size_of_val(&optval) as libc::socklen_t
    )
  };
  if result != 0 {
    return Err(io::Error::last_os_error());
  }
  Ok(())
}

fn set_send_buffer_size(fd: RawFd, size: usize) -> io::Result<()> {
  set_so_value(fd, size as libc::c_int, libc::SOL_SOCKET, libc::SO_SNDBUF)
}

fn main() {
  let mut threads = vec![];
  let mut channels = vec![];
  let connection_count = Arc::new(Mutex::new(0));

  for i in 0..NUM_THREADS {
    let (tx, rx): (Sender<Event>, Receiver<Event>) = channel();
    channels.push(tx);

    let connection_count = Arc::clone(&connection_count);
    let thread_name = format!("worker-{}", i);
    let thread_builder = thread::Builder::new().name(thread_name.clone());

    threads.push(
      thread_builder.spawn(move || {
        println!("Spawned {}", thread_name);

        let mut clients = vec![];
        let mut rng = rand::thread_rng();

        loop {
          match rx.recv() {
            Ok(Event::Client(stream)) => {
              let buffer_size = rng.gen_range(MESSAGE_SIZE_MIN..MESSAGE_SIZE_MAX);
              clients.push(Client {
                tcp_stream: stream,
                buffer: vec![0; buffer_size],
              });
              continue;
            }
            Ok(Event::Write(amount)) => {
              for _ in 0..amount {
                clients.retain_mut(|client| {
                  match client.tcp_stream.write(&client.buffer) {
                    Ok(_n) => {
                      return true;
                    }
                    Err(e) => {
                      println!(
                        "{}, Error Kind: {}, Message Size: {}",
                        e,
                        e.kind(),
                        client.buffer.len()
                      );

                      if
                        let Err(e) = set_linger(client.tcp_stream.as_raw_fd(), Some(Duration::ZERO))
                      {
                        println!("Error setting SO_LINGER: {e}");
                      }

                      let mut num = connection_count.lock().unwrap();
                      *num -= 1;
                      println!("Server: Active Connection Count: {:?}", num);
                      return false;
                    }
                  }
                });
              }
            }
            Err(_) => {
              println!("Channel disconnected. Terminating thread.");
              break;
            }
          }
        }
      })
    );
  }

  {
    let channels = channels.clone();
    threads.push(
      thread::Builder
        ::new()
        .name("write-event".into())
        .spawn(move || {
          loop {
            thread::sleep(Duration::from_millis(SLEEP_MS));

            for channel in &channels {
              channel.send(Event::Write(1)).unwrap();
            }
          }
        })
    );
  }

  let listener = TcpListener::bind(ADDRESS).unwrap();
  println!("Server Started on {}", ADDRESS);
  let mut thread_index = 0;

  loop {
    let (stream, _) = listener.accept().unwrap();
    stream.set_nodelay(true).unwrap();
    stream.set_nonblocking(true).unwrap();
    set_send_buffer_size(stream.as_raw_fd(), 4 * 1024 * 1024).unwrap();

    // Round robin distribution
    let tx = channels[thread_index % (NUM_THREADS as usize)].clone();
    tx.send(Event::Client(stream)).unwrap();
    thread_index += 1;

    let mut num = connection_count.lock().unwrap();
    *num += 1;
    println!("Server: Active Connection Count: {:?}", num);
  }
}
