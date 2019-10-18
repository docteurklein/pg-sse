use std::net::{TcpStream, TcpListener};
use std::io::{Read, Write};
use std::{thread, env};
use r2d2::{Pool, PooledConnection};
use r2d2_postgres::{TlsMode, PostgresConnectionManager};
use fallible_iterator::FallibleIterator;
use httparse::{EMPTY_HEADER, Request};
use uuid::Uuid;


fn handle_read(mut stream: &TcpStream, conn: PooledConnection<PostgresConnectionManager>) {
    let mut buf = [0u8 ;4096];
    match stream.read(&mut buf) {
        Ok(_) => {
            let mut headers = [EMPTY_HEADER; 16];
            let mut req = Request::new(&mut headers);
            req.parse(&buf).unwrap();
            match req.path {
                Some("/") => {
                    handle_error_response(stream)
                }
                Some(path) => {
                    let topic = crop(path, 1);
                    //println!("{:?}", req.headers);
                    //let lastSeenId = req.headers.into_iter().find(|header: &Header| {

                    //});
                    handle_sse_response(topic, stream, conn)
                },
                None => {
                    handle_error_response(stream)
                }
            }
        },
        Err(e) => println!("Unable to read stream: {}", e),
    }
}

fn crop(string: &str, len: usize) -> String {
    string.chars().skip(len).collect()
}

fn handle_sse_response(topic: String, mut stream: &TcpStream, conn: PooledConnection<PostgresConnectionManager>) {
    let response = b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nAccess-Control-Allow-Origin: *\r\n\r\n";
    match stream.write(response) {
        Ok(_) => println!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nAccess-Control-Allow-Origin: *\n"),
        Err(e) => println!("Failed sending response: {}", e),
    }

    println!("subscribing to topic: {}", topic);
    conn.execute(&format!("LISTEN {}", topic), &[]).unwrap();
    let notifications = conn.notifications();
    let mut it = notifications.blocking_iter();

    let get_payload = conn.prepare("select payload::text from api.events where id = $1").unwrap();

    while let Ok(Some(notification)) = it.next() {
        let id = Uuid::parse_str(&notification.payload).unwrap();
        println!("sending event id: {}", id);

        stream.write(b"event:").unwrap();
        stream.write(notification.channel.as_bytes()).unwrap();
        stream.write(b"\n").unwrap();

        stream.write(b"id:").unwrap();
        stream.write(id.hyphenated().to_string().as_bytes()).unwrap();
        stream.write(b"\n").unwrap();

        let data: String = get_payload.query(&[&id]).unwrap().get(0).get("payload");
        stream.write(b"data:").unwrap();
        stream.write(data.as_bytes()).unwrap();
        stream.write(b"\n\n").unwrap();
    }
}

fn handle_error_response(mut stream: &TcpStream) {
    let response = b"HTTP/1.1 400 Bad Request\r\n\r\n";
    match stream.write(response) {
        Ok(_) => println!("HTTP/1.1 400 Bad Request\n"),
        Err(e) => println!("Failed sending response: {}", e),
    }
}

fn main() {
    let bind_addr = env::var("BIND_ADDR").expect("BIND_ADDR");
    let pg_dsn = env::var("PG_DSN").expect("PG_DSN");

    let listener = TcpListener::bind(&bind_addr).unwrap();
    println!("Listening for connections on {}", &bind_addr);

    let pool = Pool::new(PostgresConnectionManager::new(pg_dsn, TlsMode::None).unwrap()).unwrap();

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let conn = pool.get().unwrap();
                thread::spawn(move || {
                    handle_read(&stream, conn)
                });
            }
            Err(e) => {
                println!("Unable to connect: {}", e);
            }
        }
    }
}
