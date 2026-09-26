// Make dist/escpost-chrome.zip, the archive to upload to the Chrome Web Store.
//
// The store signs the extension and gives it an identity. The `key` in
// manifests/chrome.json gives the same identity to a local unpacked install, so
// that developers and the SDK agree on it before the store exists. The upload
// must not contain that key, thus this script removes it from the copy it packs.
import { cpSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const extensionRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const built = resolve(extensionRoot, "dist/chrome");
const staged = resolve(extensionRoot, "dist/.chrome-upload");
const archive = resolve(extensionRoot, "dist/escpost-chrome.zip");

if (!existsSync(resolve(built, "manifest.json"))) {
  throw new Error("Missing dist/chrome/manifest.json. Build the extension first.");
}

rmSync(staged, { recursive: true, force: true });
mkdirSync(staged, { recursive: true });
cpSync(built, staged, { recursive: true });

const manifestPath = resolve(staged, "manifest.json");
const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
delete manifest.key;
writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);

rmSync(archive, { force: true });
// `zip` walks the current directory, so the archive holds manifest.json at its
// root. A nested folder makes the store refuse the upload.
execFileSync("zip", ["--quiet", "--recurse-paths", archive, "."], { cwd: staged });
rmSync(staged, { recursive: true, force: true });

console.log(`${manifest.name} ${manifest.version} -> dist/escpost-chrome.zip`);
