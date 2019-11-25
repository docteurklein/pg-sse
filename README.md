# pg-sse

## what ?

An http server sending postgres `LISTEN` payloads via Server-Sent-Events.


## how ?


### setup postgres

	docker-compose up -d
	export PGHOST=0 PGPORT=5432 PGUSER=api
	psql -f schema.sql


### run the server

	export PG_DSN=postgres://postgres@localhost
	export BIND_ADDR=0.0.0.0:3001

	cargo run


### listen to events

	# $BISCUIT can be one of the biscuits generated at server startup.

	curl -i 0:3001/users -H 'Last-Event-ID: 4cd99657-3d81-4527-941f-8872317741ae' -H "authorization: $BISCUIT"


### publish events

```
curl -i 0:3000/events -H 'content-type: application/json' -i -d '@-' <<-EOF
	[
		{"name": "user-registered", "topic": "users", "version": 2, "payload": {"name": "john"}, "policy": "view"},
		{"name": "user-changed-password", "topic": "users", "version": 2, "payload": {"name": "john", "policy": null}}
	]
EOF
```
