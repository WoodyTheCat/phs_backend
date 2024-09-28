create type role as enum('teacher', 'admin');

create table categories (
  id serial primary key,
  category varchar(255) not null unique
);

create table departments (
  id serial primary key,
  department varchar(255) not null unique
);

create type permission as enum(
  'edit_departments',
  'edit_categories',
  'create_posts',
  'edit_posts',
  'manage_users',
  'manage_permissions',
  'manage_pages'
);

create table users (
  id serial primary key,
  username varchar(512) not null unique,
  hash text not null, -- A PHC-format hash string of the user's password
  name varchar(512) not null,

  description text not null,
  department integer,

  role role not null default 'teacher',
  permissions permission[] not null default array[]::permission[],

  foreign key (department)
  references departments(id)
  on update cascade
  on delete set null
);

create table groups (
    id serial primary key,
    group_name varchar(128) not null unique,
    permissions permission[] not null
);

create table users_groups (
    user_id integer not null,
    group_id integer not null,

    foreign key (user_id)
    references users(id)
    on delete cascade
    on update cascade,

    foreign key (group_id)
    references groups(id)
    on delete cascade
    on update cascade
);

create table posts (
  id serial primary key,

  title varchar(255) not null,
  content text not null,

  author integer,
  date timestamptz not null default current_date,

  pinned boolean not null,
  department integer,
  category integer,

  foreign key (author)
  references users(id)
  on update cascade
  on delete set null,

  foreign key (department)
  references departments(id)
  on update cascade
  on delete set null,

  foreign key (category)
  references categories(id)
  on update cascade
  on delete set null
);

create type page_status as enum('unmodified', 'new', 'edited');

create table if not exists pages (
  id serial primary key,
  name varchar(255) unique not null,

  created_at timestamp not null default now(),
  updated_at timestamp not null default now(),

  modified page_status not null default 'new'::page_status
);

