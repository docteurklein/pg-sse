# pg-sse

## what ?

An http server sending postgres `LISTEN` payloads via Server-Sent-Events.


## how ?


### setup postgres

    psql -f schema.sql


### run the server

    export PG_DSN=postgres://postgres@localhost
    export BIND_ADDR=0.0.0.0:3000

    cargo run


### listen to events

    curl -i 0:3000/topic-to-listen -H 'Last-Event-ID: 4cd99657-3d81-4527-941f-8872317741ae'

