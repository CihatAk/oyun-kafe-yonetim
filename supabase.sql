-- Oyun Kafe Supabase şeması
-- Supabase Dashboard > SQL Editor'a yapıştırıp "Run" çalıştırın.

create table if not exists public.kafe_overview (
  id integer primary key,
  active_count integer default 0,
  idle_count integer default 0,
  total_stations integer default 0,
  vip_total integer default 0,
  busy_vip integer default 0,
  today_revenue double precision default 0,
  today_drinks double precision default 0,
  today_sessions integer default 0,
  live_estimate double precision default 0,
  low_stock_threshold integer default 5,
  campaigns_active integer default 0,
  packages_active integer default 0,
  updated_at timestamptz default now()
);

create table if not exists public.kafe_stations (
  id text primary key,
  name text not null,
  type text not null default 'standard',
  group_name text not null default '',
  status text not null default 'idle',
  customer text not null default '',
  start_time text,
  elapsed_min integer default 0,
  updated_at timestamptz default now()
);

create table if not exists public.kafe_sessions (
  station_id text primary key,
  station_name text not null,
  customer text not null default '',
  rate_type text not null default '',
  start_time text,
  is_paused boolean default false,
  minutes integer default 0,
  fee double precision default 0,
  drink_total double precision default 0,
  total double precision default 0,
  updated_at timestamptz default now()
);

create table if not exists public.kafe_drinks (
  id text primary key,
  name text not null,
  price double precision not null default 0,
  category text not null default '',
  stock integer not null default -1,
  emoji text not null default '',
  description text not null default '',
  cost double precision not null default 0,
  min_stock integer not null default -1,
  is_active integer not null default 1,
  updated_at timestamptz default now()
);

create table if not exists public.kafe_history (
  id text primary key,
  station_name text not null,
  customer text not null default '',
  start_time text,
  end_time text,
  duration_minutes integer default 0,
  total double precision default 0,
  payment_method text not null default '',
  drink_total double precision default 0,
  updated_at timestamptz default now()
);

create table if not exists public.kafe_day_end (
  id text primary key,
  sessions integer default 0,
  total_revenue double precision default 0,
  total_discount double precision default 0,
  drink_revenue double precision default 0,
  avg_duration_minutes double precision default 0,
  cash_revenue double precision default 0,
  card_revenue double precision default 0,
  other_revenue double precision default 0,
  partial_cash double precision default 0,
  partial_card double precision default 0,
  top_drinks jsonb default '[]',
  top_stations jsonb default '[]',
  updated_at timestamptz default now()
);

-- Gerçek zamanlı güncellemeler için (isteğe bağlı)
alter publication supabase_realtime add table public.kafe_overview, public.kafe_stations, public.kafe_sessions, public.kafe_drinks, public.kafe_history, public.kafe_day_end;

-- ─── GÜVENLİK: Salt-okunur panel için RLS ───
-- Şu ana kadar herkes (anon) hem okuyabilir hem YAZABİLİR.
-- Bu blok RLS'yi açar ve anon rolüne yalnızca SELECT izni verir.
-- Yazma işlemleri service_role ile devam eder (RLS'i bypass eder).

alter table public.kafe_overview  enable row level security;
alter table public.kafe_stations  enable row level security;
alter table public.kafe_sessions  enable row level security;
alter table public.kafe_drinks    enable row level security;
alter table public.kafe_history   enable row level security;
alter table public.kafe_day_end   enable row level security;

create policy "anon select kafe_overview" on public.kafe_overview for select to anon using (true);
create policy "anon select kafe_stations" on public.kafe_stations for select to anon using (true);
create policy "anon select kafe_sessions" on public.kafe_sessions for select to anon using (true);
create policy "anon select kafe_drinks"   on public.kafe_drinks   for select to anon using (true);
create policy "anon select kafe_history"  on public.kafe_history  for select to anon using (true);
create policy "anon select kafe_day_end"  on public.kafe_day_end  for select to anon using (true);

grant select on public.kafe_overview  to anon;
grant select on public.kafe_stations  to anon;
grant select on public.kafe_sessions  to anon;
grant select on public.kafe_drinks    to anon;
grant select on public.kafe_history   to anon;
grant select on public.kafe_day_end   to anon;
