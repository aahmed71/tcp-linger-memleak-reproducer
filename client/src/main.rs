#![allow(unused_imports)]

use tokio::io::{ AsyncBufReadExt, AsyncReadExt, AsyncWriteExt };
use tokio::net::TcpStream;
use tokio::runtime::Builder;
use tokio::time::{ sleep, Duration };

const ADDRESS: &str = "127.0.0.1:6666";
const CONNECTIONS: u16 = 25_000;

async fn spawn_connection() {
  match TcpStream::connect(ADDRESS).await {
    Ok(stream) => {
      stream.set_nodelay(true).unwrap();
      println!("Connection established!");
      if let Err(err) = handle_connection(stream).await {
        eprintln!("Error handling connection: {}", err);
      }
    }
    Err(err) => {
      eprintln!("Error connecting to server: {}", err);
    }
  }
}

async fn handle_connection(mut stream: TcpStream) -> Result<(), std::io::Error> {
  let mut buffer = [0; 10240];

  loop {
    let n = stream.read(&mut buffer).await?;
    if n == 0 {
      println!("Connection closed by peer");
      break;
    }
  }
  Ok(())
}

fn main() {
  let rt = Builder::new_multi_thread().enable_all().build().unwrap();
  rt.block_on(async {
    let mut handles = Vec::new();

    for _ in 0..CONNECTIONS {
      let handle = tokio::spawn(async move {
        spawn_connection().await;
      });

      handles.push(handle);
      sleep(Duration::from_micros(100)).await;
    }

    for handle in handles {
      handle.await.unwrap();
    }
  });

  println!("All connections closed");
}
