alter table users add column sessions text[] not null default array[]::text[];
