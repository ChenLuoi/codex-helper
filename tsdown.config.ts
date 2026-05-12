import { defineConfig } from "tsdown";

export default defineConfig({
  entry: {
    cli: "src/cli.ts",
    index: "src/index.ts"
  },
  clean: true,
  dts: true,
  format: ["esm"],
  hash: false,
  minify: false,
  outDir: "dist",
  sourcemap: true,
  target: "node20"
});
