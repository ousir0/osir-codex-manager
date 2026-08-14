import react from "@vitejs/plugin-react";
import type { Plugin } from "vite";
import { configDefaults, defineConfig } from "vitest/config";

import pkg from "./package.json";

const SKIN_CATALOG_BASES = [
  "https://raw.githubusercontent.com/qq501987847/codex-app-manager-skins/main",
  "https://gitee.com/qq501987849/codex-app-manager-skins/raw/master",
];

const skinCatalogProxy: Plugin = {
  name: "skin-catalog-dev-proxy",
  configureServer(server) {
    server.middlewares.use("/__skins", async (request, response) => {
      const relativePath = (request.url ?? "").split("?", 1)[0].replace(/^\/+/, "");
      const safe =
        relativePath.length > 0 &&
        !relativePath.includes("..") &&
        /^[a-z0-9/_\-.]+$/i.test(relativePath);
      if (!safe) {
        response.statusCode = 400;
        response.end("invalid skin catalog path");
        return;
      }

      for (const base of SKIN_CATALOG_BASES) {
        try {
          const upstream = await fetch(`${base}/${relativePath}`, {
            signal: AbortSignal.timeout(5_000),
          });
          if (!upstream.ok) continue;
          const body = Buffer.from(await upstream.arrayBuffer());
          response.statusCode = 200;
          response.setHeader(
            "content-type",
            upstream.headers.get("content-type") ?? "application/octet-stream",
          );
          response.setHeader("cache-control", "no-store");
          response.end(body);
          return;
        } catch {
          // Try the next pinned source.
        }
      }

      response.statusCode = 502;
      response.end("skin catalog sources unavailable");
    });
  },
};

export default defineConfig({
  plugins: [react(), skinCatalogProxy],
  define: {
    "import.meta.env.VITE_APP_VERSION": JSON.stringify(pkg.version),
  },
  clearScreen: false,
  test: {
    exclude: [...configDefaults.exclude, ".claude/**"],
    environment: "jsdom",
    environmentOptions: {
      jsdom: {
        url: "http://localhost/",
      },
    },
    setupFiles: ["./vitest.setup.ts"],
    globals: false,
  },
  server: {
    host: "127.0.0.1",
    port: 1420,
    strictPort: true,
  },
});
