import react from "@vitejs/plugin-react";
import UnoCSS from "unocss/vite";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { defineConfig, type Plugin } from "vite";
import { otoolsTauriShimPlugin } from "./vendor/otools-plugin-sdk/src/vite";

function patchOcradSource(code: string) {
  return code
    .replace(/\b0([0-7]{3})\b/g, "0o$1")
    .replace(/this\[['"]Module['"]\]\s*=\s*Module;/g, 'globalThis["Module"] = Module;');
}

function ocradOptimizeDepsPlugin() {
  return {
    name: "ocrad-optimize-deps",
    setup(build: {
      onLoad: (
        options: { filter: RegExp },
        callback: (args: { path: string }) => Promise<{ contents: string; loader: "js" }>,
      ) => void;
    }) {
      build.onLoad({ filter: /ocrad\.js[\\/]ocrad\.js$/ }, async (args) => ({
        contents: patchOcradSource(await readFile(args.path, "utf8")),
        loader: "js",
      }));
    },
  };
}

function ocradLegacyOctalPlugin(): Plugin {
  return {
    name: "ocrad-legacy-octal",
    enforce: "pre",
    transform(code, id) {
      const normalizedId = id.split("?")[0].replace(/\\/g, "/");
      if (!normalizedId.endsWith("/ocrad.js") && !normalizedId.includes("/.vite/deps/ocrad__js.js")) {
        return null;
      }
      return { code: patchOcradSource(code), map: null };
    },
  };
}

export default defineConfig({
  base: "./",
  define: { __OTOOLS_PLUGIN_UUID__: JSON.stringify("otools-ai-switch") },
  // The official plugin aliases to SDK package subpaths. Resolve those to the
  // pinned source copy so this build does not require a prebuilt SDK package.
  plugins: [
    otoolsTauriShimPlugin(),
    {
      name: "otools-vendored-source",
      enforce: "pre",
      resolveId(id) {
        if (id.startsWith("otools-plugin-sdk/")) {
          return fileURLToPath(new URL(`./vendor/otools-plugin-sdk/src/${id.slice("otools-plugin-sdk/".length)}.ts`, import.meta.url));
        }
      },
    },
    ocradLegacyOctalPlugin(), UnoCSS(), react(),
  ],
  clearScreen: false,
  server: { host: "127.0.0.1", port: 1420, strictPort: true },
  optimizeDeps: { esbuildOptions: { plugins: [ocradOptimizeDepsPlugin()] } },
  build: { target: "es2020" },
});
