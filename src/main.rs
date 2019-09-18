
#[macro_use] extern crate lazy_static;

use hyper_sse::Server;
use postgres::{Connection, TlsMode};
use fallible_iterator::FallibleIterator;

lazy_static! {
    static ref SSE: Server<u8> = Server::new();
}

fn main() {
    SSE.spawn("0.0.0.0:3000".parse().unwrap());

    let auth_token = SSE.generate_auth_token(Some(0)).unwrap();
    println!("http://0.0.0.0:3000/push/0?{}", auth_token);

    let conn = Connection::connect("postgres://postgres@localhost", TlsMode::None).unwrap();
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
