begin;

drop schema if exists api cascade;
create schema api;

drop schema if exists internal cascade;
create schema internal;

create extension if not exists "uuid-ossp" with schema internal;

create table api.events (
    id uuid primary key default internal.uuid_generate_v4(),
    name text not null,
    topic text not null,
    version text not null,
    added_at timestamptz not null default clock_timestamp(),
    payload jsonb not null,
    policy jsonb
);

create or replace function internal.notify_event() returns trigger language plpgsql as $$
begin
    perform pg_notify(new.topic, new.id::text);
    return null;
end;
$$;
create trigger on_event_insert after insert on api.events
for each row execute function internal.notify_event();

drop role if exists anon;
create role anon nologin;

grant usage on schema api to anon;
grant select on api.events to anon;
grant insert (name, topic, version, payload, policy) on api.events to anon;
grant anon to api;

commit;
