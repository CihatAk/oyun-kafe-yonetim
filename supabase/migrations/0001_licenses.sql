-- JiJi Game Center lisans tablosu
create table if not exists public.licenses (
  key text primary key,
  license_id text not null unique,
  business_name text not null default '',
  machine_hash text,
  status text not null default 'available'
    check (status in ('available', 'activated', 'revoked')),
  created_at timestamptz not null default now(),
  activated_at timestamptz,
  note text not null default ''
);

alter table public.licenses enable row level security;
