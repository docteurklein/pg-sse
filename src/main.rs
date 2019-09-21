use std::net::{TcpStream, TcpListener};
use std::io::{Read, Write};
use std::{thread, env};
use r2d2::{Pool, PooledConnection};
use r2d2_postgres::{TlsMode,PostgresConnectionManager};
use fallible_iterator::FallibleIterator;


fn handle_read(mut stream: &TcpStream) {
    let mut buf = [0u8 ;4096];
    match stream.read(&mut buf) {
        Ok(_) => {
            let req_str = String::from_utf8_lossy(&buf);
            println!("{}", req_str);
            },
        Err(e) => println!("Unable to read stream: {}", e),
    }
}

fn handle_write(mut stream: TcpStream, conn: PooledConnection<PostgresConnectionManager>) {
    let response = b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream; charset=UTF-8\r\n\r\n";
    match stream.write(response) {
        Ok(_) => println!("Response sent"),
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
    handle_read(&stream);
    handle_write(stream, conn);
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
                    conn.execute(&format!("LISTEN {}", &"test"), &[]).unwrap();
                    handle_client(stream, conn)
                });
            }
            Err(e) => {
                println!("Unable to connect: {}", e);
            }
        }
    }
}
