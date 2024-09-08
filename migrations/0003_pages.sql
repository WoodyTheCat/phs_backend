create type page_status as enum('unmodified', 'new', 'edited');

create table if not exists pages (
  id serial primary key,
  name varchar(255) unique not null,
  
  created_at timestamp not null default now(),
  updated_at timestamp not null default now(),

  modified page_status not null default 'new'::page_status
);
