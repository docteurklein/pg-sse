use std::net::{TcpStream, TcpListener};
use std::{io, io::{Read, Write}};
use std::{thread, env, env::{VarError}};
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
                    let topic = path.chars().skip(1).collect();
                    let last_event_id = {
                        match req.headers.into_iter().find(|header| header.name.to_lowercase() == "last-event-id") {
                            Some(header) => {
                                Some(Uuid::parse_str(&std::str::from_utf8(header.value).unwrap().to_string()).unwrap())
                            }
                            None => None
                        }
                    };
                    let auth_token = {
                        match req.headers.into_iter().find(|header| header.name.to_lowercase() == "authorization") {
                            Some(header) => {
                                Some(std::str::from_utf8(header.value).unwrap())
                            }
                            None => None
                        }
                    };
                    handle_sse_response(topic, stream, conn, last_event_id, auth_token)
                },
                None => {
                    handle_error_response(stream)
                }
            }
        },
        Err(e) => println!("Unable to read stream: {}", e),
    }
}

fn handle_sse_response(topic: String, mut stream: &TcpStream, conn: PooledConnection<PostgresConnectionManager>, last_event_id: Option<Uuid>, auth_token: Option<&str>) {
    let response = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nAccess-Control-Allow-Origin: *\r\n\r\n";
    match stream.write(response.as_bytes()) {
        Ok(_) => println!("{}", response),
        Err(e) => println!("Failed sending response: {}", e),
    }

    match last_event_id {
        Some(id) => {
            println!("Sending events after Last-Event-ID: {}", id);
            let get_missed_events = conn.query("select id, name, policy::text, row_to_json(event)::text as data from api.events event \
                where topic = $1 \
                and added_at >= (select added_at from api.events where id = $2)\
                and id != $2\
                order by added_at asc", &[
                    &topic,
                    &id,
                ]
            ).unwrap();

            for row in &get_missed_events {
                if can_view(auth_token, row.get("policy")) {
                    send_event(stream, row.get("name"), row.get("id"), row.get("data"));
                }
            }
        }
        None => println!("no Last-Event-ID"),
    }

    println!("subscribing to topic: {}", topic);
    conn.execute(&format!("LISTEN {}", topic), &[]).unwrap();
    let notifications = conn.notifications();
    let mut it = notifications.blocking_iter();

    let get_payload = conn.prepare("select id, name, policy::text, row_to_json(event)::text as data from api.events event where id = $1").unwrap();

    while let Ok(Some(notification)) = it.next() {
        let id = Uuid::parse_str(&notification.payload).unwrap();
        for row in &get_payload.query(&[&id]).unwrap() {
            if can_view(auth_token, row.get("policy")) {
                send_event(stream, row.get("name"), id, row.get("data"));
            }
        }
    }
}

fn can_view(auth_token: Option<&str>, policy: Option<String>) -> bool {
    match policy {
        Some(policy_value) => {
            match auth_token {
                Some(value) => {
                    format!("\"{}\"", value) == policy_value // @todo use biscuits
                }
                None => false
            }
        }
        None => true
    }
}

fn send_event(mut stream: &TcpStream, name: String, id: Uuid, data: String) {
    println!("sending event id: {}", id);

    stream.write(b"event:").unwrap();
    stream.write(name.as_bytes()).unwrap();
    stream.write(b"\n").unwrap();

    stream.write(b"id:").unwrap();
    stream.write(id.hyphenated().to_string().as_bytes()).unwrap();
    stream.write(b"\n").unwrap();

    stream.write(b"data:").unwrap();
    stream.write(data.as_bytes()).unwrap();
    stream.write(b"\n\n").unwrap();
}

fn handle_error_response(mut stream: &TcpStream) {
    let response = "HTTP/1.1 400 Bad Request\r\n\r\n";
    match stream.write(response.as_bytes()) {
        Ok(_) => println!("{}", response),
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
                match pool.get() {
                    Ok(conn) => {
                        thread::spawn(move || {
                            handle_read(&stream, conn)
                        });
                    }
                    Err(e) => {
                        println!("Unable to get connection from pool: {}", e);
                    }
                }
            }
            Err(e) => {
                println!("Unable to connect: {}", e);
            }
        }
    };
}

#[derive(Debug)]
pub enum Error {
    IoError(io::Error),
    VarError(VarError),
}

impl From<VarError> for Error {
    fn from(e: VarError) -> Error {
        Error::VarError(e)
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Error {
        Error::IoError(e)
    }
}
