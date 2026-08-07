-- JiJi Game Center lisans takip sistemi (v2.0.9 sonrasi)
-- 1) licenses tablosuna expires_at sutunu ekle
alter table public.licenses add column if not exists expires_at timestamptz;

-- 2) Musteriler tablosu
create table if not exists public.customers (
  id uuid primary key default gen_random_uuid(),
  name text not null,
  phone text not null default '',
  whatsapp text not null default '',
  email text not null default '',
  address text not null default '',
  notes text not null default '',
  created_at timestamptz not null default now()
);

-- 3) Odemeler tablosu
create table if not exists public.payments (
  id uuid primary key default gen_random_uuid(),
  customer_id uuid references public.customers(id) on delete set null,
  license_key text references public.licenses(key) on delete set null,
  amount numeric(12,2) not null default 0,
  payment_date date not null default current_date,
  description text not null default '',
  created_at timestamptz not null default now()
);

-- 4) Faturalar tablosu
create table if not exists public.invoices (
  id uuid primary key default gen_random_uuid(),
  invoice_no text not null unique,
  customer_id uuid references public.customers(id) on delete set null,
  license_key text references public.licenses(key) on delete set null,
  amount numeric(12,2) not null default 0,
  vat numeric(12,2) not null default 0,
  total numeric(12,2) not null default 0,
  status text not null default 'open' check (status in ('open', 'paid')),
  created_at timestamptz not null default now()
);

alter table public.customers enable row level security;
alter table public.payments enable row level security;
alter table public.invoices enable row level security;
