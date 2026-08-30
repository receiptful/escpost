/**
 * The QZ compatible surface, injected into a granted page.
 *
 * The surface lives in `@escpost/browser`, so the injected copy and the
 * drop-in a developer can import are one implementation. This file is only an
 * injection entry point: content scripts load as classic scripts, so the build
 * bundles the import away.
 *
 * It calls the installer rather than importing the drop-in for its side
 * effect, because a side effect is exactly what a bundler is entitled to
 * remove.
 */
import { installQzShim } from "../../packages/browser/src/qz/surface";

installQzShim();
