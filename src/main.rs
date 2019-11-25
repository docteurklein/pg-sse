use std::net::{TcpStream, TcpListener};
use std::{io::{Read, Write}};
use std::{thread, env};
use r2d2::{Pool, PooledConnection};
use r2d2_postgres::{TlsMode, PostgresConnectionManager};
use fallible_iterator::FallibleIterator;
use httparse::{EMPTY_HEADER, Request};
use uuid::Uuid;
use biscuit::{crypto::{KeyPair,PrivateKey}, token::{Biscuit, builder::*}};
use rand::{self, rngs::ThreadRng};
use base64;
use hex;
use json;

fn handle_read(root: &KeyPair, mut stream: &TcpStream, conn: PooledConnection<PostgresConnectionManager>) {
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
                                Some(Biscuit::from(&base64::decode_config(header.value, base64::URL_SAFE_NO_PAD).unwrap()).unwrap())
                            }
                            None => None
                        }
                    };
                    handle_sse_response(root, topic, stream, conn, last_event_id, auth_token)
                },
                None => {
                    handle_error_response(stream)
                }
            }
        },
        Err(e) => println!("Unable to read stream: {}", e),
    }
}

fn handle_sse_response(root: &KeyPair, topic: String, mut stream: &TcpStream, conn: PooledConnection<PostgresConnectionManager>, last_event_id: Option<Uuid>, auth_token: Option<Biscuit>) {
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
                if can_view(root, auth_token.clone(), row.get("policy")) {
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

    let get_event = conn.prepare("select id, name, policy::text, row_to_json(event)::text as data from api.events event where id = $1").unwrap();

    while let Ok(Some(notification)) = it.next() {
        let id = Uuid::parse_str(&notification.payload).unwrap();
        for row in &get_event.query(&[&id]).unwrap() {
            if can_view(root, auth_token.clone(), row.get("policy")) {
                send_event(stream, row.get("name"), id, row.get("data"));
            }
        }
    }
}

fn can_view(root: &KeyPair, auth_token: Option<Biscuit>, policy: Option<String>) -> bool {
    match policy {
        Some(policy_value) => {
            match auth_token {
                Some(biscuit) => {
                    println!("{} VS {}", biscuit.print(), policy_value);
                    let mut verif = biscuit.verify(root.public()).unwrap();
                    verif.add_resource("event");
                    match json::parse(&policy_value) {
                        Ok(p) => {
                            verif.add_operation(p.as_str().unwrap());
                        }
                        Err(e) => {
                            eprintln!("invalid json policy '{}': {}", &policy_value, e);
                        }
                    }

                    verif.verify().is_ok()
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
    let mut rng = rand::thread_rng();

    let newroot = KeyPair::new(&mut rng);
    println!("new root secret key: {:?}", hex::encode(&newroot.private().to_bytes()));

    let mut bytes = [0u8; 32];
    hex::decode_to_slice(&env::var("SECRET").expect("SECRET"), &mut bytes as &mut [u8]).unwrap();
    let root1 = KeyPair::from(PrivateKey::from_bytes(bytes).unwrap());
    biscuits(&root1, &mut rng);

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
                            let root2 = KeyPair::from(PrivateKey::from_bytes(bytes).unwrap());
                            handle_read(&root2, &stream, conn)
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

fn biscuits(root: &KeyPair, rng: &mut ThreadRng) {
    let base_token = {
        let mut builder = Biscuit::builder(rng, root);

        // every fact added to the authority block must have the authority fact
        builder.add_authority_fact(&fact("right", &[s("authority"), string("event"), s("view")]));
        let biscuit = builder.build().unwrap();
        biscuit.to_vec().unwrap()
    };

    let view_token = {
        let deser = Biscuit::from(&base_token).unwrap();
        let mut builder = deser.create_block();

        // caveats are implemented as logic rules. If the rule produces something,
        // the caveat is successful
        builder.add_caveat(&rule(
            // the rule's name
            "caveat",
            // the "head" of the rule, defining the kind of result that is produced
            &[s("resource")],
            // here we require the presence of a "resource" fact with the "ambient" tag
            // (meaning it is provided by the verifier)
            &[
                pred("resource", &[s("ambient"), string("event")]),
                // we restrict to view operations
                pred("operation", &[s("ambient"), s("view")]),
            ],
        ));

        let keypair = KeyPair::new(rng);
        deser.append(rng, &keypair, builder.build()).unwrap().to_vec().unwrap()
    };

    let nope_token = {
        let deser = Biscuit::from(&base_token).unwrap();
        let mut builder = deser.create_block();

        // caveats are implemented as logic rules. If the rule produces something,
        // the caveat is successful
        builder.add_caveat(&rule(
            // the rule's name
            "caveat",
            // the "head" of the rule, defining the kind of result that is produced
            &[s("resource")],
            // here we require the presence of a "resource" fact with the "ambient" tag
            // (meaning it is provided by the verifier)
            &[
                pred("resource", &[s("ambient"), string("SOMETHING_ELSE")]),
                // we restrict to view operations
                pred("operation", &[s("ambient"), s("view")]),
            ],
        ));

        let keypair = KeyPair::new(rng);
        deser.append(rng, &keypair, builder.build()).unwrap().to_vec().unwrap()
    };

    println!("view: {}", base64::encode_config(&view_token, base64::URL_SAFE_NO_PAD));
    println!("nope: {}", base64::encode_config(&nope_token, base64::URL_SAFE_NO_PAD));
}
