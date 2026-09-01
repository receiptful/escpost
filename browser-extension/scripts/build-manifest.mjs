import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const extensionRoot = resolve(here, "..");
const dist = resolve(extensionRoot, "dist");

copyFileSync(resolve(extensionRoot, "manifest.json"), resolve(dist, "manifest.json"));
copyFileSync(
  resolve(extensionRoot, "src/popup/popup.html"),
  resolve(dist, "popup.html"),
);

const icons = [16, 32, 48, 128];
mkdirSync(resolve(dist, "icons"), { recursive: true });
for (const size of icons) {
  copyFileSync(
    resolve(extensionRoot, `src/ui/icons/icon-${size}.png`),
    resolve(dist, `icons/icon-${size}.png`),
  );
}
