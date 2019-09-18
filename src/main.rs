
#[macro_use] extern crate lazy_static;

use hyper_sse::Server;
use postgres::{Connection, TlsMode};
use fallible_iterator::FallibleIterator;
use std::env;

lazy_static! {
    static ref SSE: Server<u8> = Server::new();
}

fn main() {
    let bind_addr = env::var("BIND_ADDR").expect("BIND_ADDR");
    let pg_dsn = env::var("PG_DSN").expect("PG_DSN");
    SSE.spawn(bind_addr.parse().unwrap());

    let auth_token = SSE.generate_auth_token(Some(0)).unwrap();
    println!("http://{}/push/0?{}", bind_addr, auth_token);

    let conn = Connection::connect(pg_dsn, TlsMode::None).unwrap();
    conn.execute(&format!("LISTEN {}", &"test"), &[]).unwrap();

    let notifications = conn.notifications();
    let mut it = notifications.blocking_iter();

    #[allow(while_true)]
    while true {
        match it.next() {
            Ok(Some(notification)) => {
                println!("{:?}", notification);
                SSE.push(0, &notification.channel, &notification.payload).ok();
            },
            Err(err) => eprintln!("Got err {:?}", err),
            _ => panic!("Unexpected state.")
        }
    }
}
