// JiJi Game Center lisans anahtar çifti üretici (Node 20+)
//
// Kullanım: node scripts/gen-license-keys.mjs
// Çıktı:
//   LICENSE_PRIVATE_KEY   → Supabase edge function env değişkenine yazılır (GİZLİ, kimseyle paylaşmayın)
//   LICENSE_PUBLIC_KEY    → uygulamaya (license.rs DEFAULT_PUBLIC_KEY_HEX) gömülür
import { webcrypto as crypto } from "node:crypto";

const kp = await crypto.subtle.generateKey({ name: "Ed25519" }, true, ["sign", "verify"]);
const skDer = await crypto.subtle.exportKey("pkcs8", kp.privateKey);
const pkRaw = await crypto.subtle.exportKey("raw", kp.publicKey);

const b64 = (buf) => Buffer.from(buf).toString("base64");
const hex = (buf) => Buffer.from(buf).toString("hex");

console.log("LICENSE_PRIVATE_KEY (pkcs8 base64 - SUPABASE ENV'e koyun):");
console.log(b64(skDer));
console.log("");
console.log("LICENSE_PUBLIC_KEY (raw hex 32 bayt - license.rs DEFAULT_PUBLIC_KEY_HEX):");
console.log(hex(pkRaw));
