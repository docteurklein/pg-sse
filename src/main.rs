use std::net::{TcpStream, TcpListener};
use std::io::{Read, Write};
use std::{thread, env};
use r2d2::{Pool, PooledConnection};
use r2d2_postgres::{TlsMode, PostgresConnectionManager};
use fallible_iterator::FallibleIterator;
use httparse::{EMPTY_HEADER, Request};


fn handle_read(mut stream: &TcpStream, conn: PooledConnection<PostgresConnectionManager>) {
    let mut buf = [0u8 ;4096];
    match stream.read(&mut buf) {
        Ok(_) => {
            let mut headers = [EMPTY_HEADER; 16];
            let mut req = Request::new(&mut headers);
            let result = req.parse(&buf).unwrap();
            match req.path {
                Some(path) => {
                    let topic = crop(path, 1);
                    println!("LISTEN {}", topic);
                    conn.execute(&format!("LISTEN {}", topic), &[]).unwrap();
                    handle_write(stream, conn)
                },
                None => {
                    // must read more and parse again
                }
            }
        },
        Err(e) => println!("Unable to read stream: {}", e),
    }
}

fn crop(string: &str, len: usize) -> String {
    string.chars().skip(len).collect()
}

fn handle_write(mut stream: &TcpStream, conn: PooledConnection<PostgresConnectionManager>) {
    let response = b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream; charset=UTF-8\r\n\r\n";
    match stream.write(response) {
        Ok(_) => println!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream; charset=UTF-8\r\n\r\n"),
        Err(e) => println!("Failed sending response: {}", e),
    }

    let notifications = conn.notifications();
    let mut it = notifications.blocking_iter();

    while let Ok(Some(notification)) = it.next() {
        stream.write(b"event:").unwrap();
        stream.write(notification.channel.as_bytes()).unwrap();
        stream.write(b"\n").unwrap();
        stream.write(b"data:").unwrap();
        stream.write(notification.payload.as_bytes()).unwrap();
        stream.write(b"\n\n").unwrap();
    }
}

fn handle_client(stream: TcpStream, conn: PooledConnection<PostgresConnectionManager>) {
    handle_read(&stream, conn);
}

fn main() {
    let bind_addr = env::var("BIND_ADDR").expect("BIND_ADDR");
    let pg_dsn = env::var("PG_DSN").expect("PG_DSN");

    let listener = TcpListener::bind(&bind_addr).unwrap();
    println!("Listening for connections on {}", &bind_addr);

    let pool = Pool::new(PostgresConnectionManager::new(pg_dsn, TlsMode::None).unwrap()).unwrap();
    for stream in listener.incoming() {
        let conn = pool.get().unwrap();
        match stream {
            Ok(stream) => {
                thread::spawn(move || {
                    handle_client(stream, conn)
                });
            }
            Err(e) => {
                println!("Unable to connect: {}", e);
            }
        }
    }
}
