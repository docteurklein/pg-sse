# pg-sse

## what ?

An http server sending postgres `LISTEN` payloads via Server-Sent-Events.


## how ?

    export PG_DSN=postgres://postgres@localhost
    export BIND_ADDR=0.0.0.0:3000

    cargo run
