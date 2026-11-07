/**
 * Entry point of `escpost-qz.js`, the build for a `<script>` tag.
 *
 * It exports nothing on purpose: an export would make the IIFE build assign
 * its exports to a global, and the only global a QZ page needs is `window.qz`.
 */
import { installQzShim } from "./surface";

installQzShim();
