// JiJi Game Center lisans sunucusu (Supabase Edge Function)
//
// Uçlar:
//   POST /license/generate   (x-admin-token: LICENSE_ADMIN_TOKEN) — lisans anahtarı üretir
//   POST /license/activate   { key, machine_hash } — anahtarı makineye bağlar, imzalı token döndürür
//   POST /license/check      { license_id, machine_hash } — geçerlilik/iptal durumu
//
// Ortam değişkenleri:
//   LICENSE_PRIVATE_KEY   — Ed25519 özel anahtarı (PKCS#8 DER, base64). scripts/gen-license-keys.mjs üretir.
//   LICENSE_ADMIN_TOKEN   — /generate için gizli yönetici anahtarı

import { createClient } from "https://esm.sh/@supabase/supabase-js@2";

const corsHeaders = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Headers": "authorization, x-client-info, apikey, content-type, x-admin-token",
};

function json(data, status = 200) {
  return new Response(JSON.stringify(data), {
    status,
    headers: { ...corsHeaders, "Content-Type": "application/json" },
  });
}

function ok(data) { return json(data); }
function error(msg, status = 400) { return json({ error: msg }, status); }

function bytesToHex(bytes) {
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

function base64ToBytes(b64) {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

async function signToken(payload) {
  const privateKeyB64 = Deno.env.get("LICENSE_PRIVATE_KEY") || "";
  if (!privateKeyB64) throw new Error("LICENSE_PRIVATE_KEY ayarlı değil");
  const key = await crypto.subtle.importKey(
    "pkcs8",
    base64ToBytes(privateKeyB64),
    { name: "Ed25519" },
    false,
    ["sign"],
  );
  const msg = new TextEncoder().encode(JSON.stringify(payload));
  const sig = await crypto.subtle.sign("Ed25519", key, msg);
  return bytesToHex(msg) + "." + bytesToHex(new Uint8Array(sig));
}

function randomCode() {
  const alphabet = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
  const part = (n) => Array.from({ length: n }, () => alphabet[Math.floor(Math.random() * alphabet.length)]).join("");
  return "JGJC-" + part(5) + "-" + part(5) + "-" + part(5);
}

function randomId() {
  const alphabet = "abcdefghijklmnopqrstuvwxyz0123456789";
  return "L-" + Array.from({ length: 12 }, () => alphabet[Math.floor(Math.random() * alphabet.length)]).join("");
}

Deno.serve(async (req) => {
  if (req.method === "OPTIONS") return new Response("ok", { headers: corsHeaders });
  const url = new URL(req.url);
  const path = url.pathname.split("/").pop();

  const supabase = createClient(
    Deno.env.get("SUPABASE_URL")!,
    Deno.env.get("SUPABASE_SERVICE_ROLE_KEY")!,
  );

  try {
    if (path === "generate") {
      if (req.headers.get("x-admin-token") !== (Deno.env.get("LICENSE_ADMIN_TOKEN") || "")) {
        return error("Yetkisiz", 403);
      }
      const body = await req.json().catch(() => ({}));
      const business_name = String(body.business_name || "İşletme").trim().slice(0, 80);
      const count = Math.min(Math.max(Number(body.count) || 1, 1), 100);
      const expires_at = body.expires_at ? String(body.expires_at).trim() : null;
      const keys = [];
      for (let i = 0; i < count; i++) {
        const key = randomCode();
        const license_id = randomId();
        const { error: err } = await supabase
          .from("licenses")
          .insert({ key, license_id, business_name, status: "available", expires_at });
        if (err) return error("Veritabanı hatası: " + err.message, 500);
        keys.push({ key, license_id, expires_at });
      }
      return ok({ keys });
    }

    if (path === "activate") {
      const body = await req.json().catch(() => ({}));
      const key = String(body.key || "").trim().toUpperCase();
      const machine_hash = String(body.machine_hash || "").trim();
      if (!key || !machine_hash) return error("key ve machine_hash zorunludur");
      const { data, error: err } = await supabase
        .from("licenses")
        .select("*")
        .eq("key", key)
        .maybeSingle();
      if (err) return error("Veritabanı hatası: " + err.message, 500);
      if (!data) return error("Geçersiz lisans anahtarı", 404);
      if (data.status === "revoked") return error("Bu lisans iptal edilmiş", 403);
      if (data.status === "activated" && data.machine_hash !== machine_hash) {
        return error("Bu anahtar başka bir bilgisayara bağlanmış", 403);
      }
      const activated_at = new Date().toISOString();
      if (data.status !== "activated") {
        const { error: upErr } = await supabase
          .from("licenses")
          .update({ status: "activated", machine_hash, activated_at })
          .eq("key", key);
        if (upErr) return error("Güncelleme hatası: " + upErr.message, 500);
      }
      const payload = {
        v: 1,
        license_id: data.license_id,
        business_name: data.business_name,
        machine_hash,
        issued_at: data.activated_at || activated_at,
        expires_at: data.expires_at || null,
      };
      const token = await signToken(payload);
      return ok({ token, license_id: data.license_id });
    }

    if (path === "check") {
      const body = await req.json().catch(() => ({}));
      const license_id = String(body.license_id || "").trim();
      const machine_hash = String(body.machine_hash || "").trim();
      if (!license_id) return error("license_id zorunludur");
      const { data, error: err } = await supabase
        .from("licenses")
        .select("status, machine_hash")
        .eq("license_id", license_id)
        .maybeSingle();
      if (err) return error("Veritabanı hatası: " + err.message, 500);
      if (!data) return error("Lisans bulunamadı", 404);
      if (data.status === "revoked") return ok({ valid: false, reason: "revoked" });
      if (data.machine_hash && machine_hash && data.machine_hash !== machine_hash) {
        return ok({ valid: false, reason: "machine_mismatch" });
      }
      return ok({ valid: true });
    }

    return error("Bilinmeyen işlem", 404);
  } catch (e) {
    return error(String(e), 500);
  }
});
