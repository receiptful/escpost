/**
 * Emits dist/manifest.json, retargeted at whatever ESCPOST_API_BASE names.
 *
 * manifest.json in the repo is always the shippable one: production hosts, no
 * localhost. A dev build does not edit it — this script rewrites the two places
 * the API origin appears on the way to dist/, so the checked-in manifest never
 * carries a host permission the Web Store would question.
 *
 * The two places, and why both must move together: `host_permissions` is what
 * lets the service worker fetch the API at all, and the auth-bridge content
 * script's `matches` is what lets the verify page hand the session token over.
 * Change one without the other and sign-in fails in a way that looks like a
 * server bug.
 */
import { readFileSync, writeFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const PRODUCTION = "https://api.receiptful.io";

const base = (process.env["ESCPOST_API_BASE"] ?? PRODUCTION).replace(/\/$/, "");
const manifest = JSON.parse(readFileSync(resolve(HERE, "../manifest.json"), "utf8"));

if (base !== PRODUCTION) {
  const retarget = (pattern) => pattern.replace(PRODUCTION, base);
  manifest.host_permissions = manifest.host_permissions.map(retarget);
  for (const script of manifest.content_scripts) {
    script.matches = script.matches.map(retarget);
  }
}

writeFileSync(resolve(HERE, "../dist/manifest.json"), JSON.stringify(manifest, null, 2) + "\n");
console.log(`manifest.json -> dist/ (API base: ${base})`);
