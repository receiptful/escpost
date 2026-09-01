import { defineConfig } from "vite";

export default defineConfig({
  build: {
    lib: {
      entry: "src/types.ts",
      formats: ["es"],
      fileName: "index",
    },
    minify: false,
  },
});
