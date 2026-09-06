import { readFileSync } from "node:fs";
import { join } from "node:path";
import { runInNewContext } from "node:vm";
import ts from "typescript";
import { describe, expect, it, vi } from "vitest";

const source = readFileSync(join(process.cwd(), "src-tauri/resources/opencodex/policy-fallback.ts"), "utf8");
const compiled = ts.transpileModule(source, {
  compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
}).outputText;

function loadPolicy(core, overload = () => null) {
  const exports = {};
  runInNewContext(compiled, {
    exports,
    require: (name) => {
      if (name === "./core") return { handleResponses: core };
      if (name === "./pacing-overload") return { requestPacingOverloadResponse: overload };
      throw new Error(`Unexpected dependency: ${name}`);
    },
  });
  return exports;
}

describe("installed OpenCodex selected route policy", () => {
  it.each([401, 403, 404, 408, 429, 500, 502, 503, 504])("returns %i without invoking another provider", async (status) => {
    const response = new Response(JSON.stringify({ error: "original upstream error" }), { status });
    const trace = { routeKind: "policy", profile: "multi", selected: { provider: "current", model: "first" },
      candidates: [{ provider: "current", model: "first", eligible: true, exclusions: [] },
        { provider: "other", model: "healthy", eligible: true, exclusions: [] }] };
    const log = { routeDecision: trace, requestedModel: "multi" };
    const config = { defaultProvider: "current", providers: { current: { baseUrl: "https://current.test/v1" } } };
    const original = structuredClone({ config, log });
    const req = new Request("http://localhost/v1/responses", { method: "POST", body: JSON.stringify({ model: "multi" }) });
    const options = { onRequestBodyRead: vi.fn() };
    const core = vi.fn().mockResolvedValue(response);
    const policy = loadPolicy(core);
    expect(await policy.handleResponses(req, config, log, options)).toBe(response);
    expect(core).toHaveBeenCalledExactlyOnceWith(req, config, log, options);
    expect({ config, log }).toEqual(original);
    expect(await response.json()).toEqual({ error: "original upstream error" });
    expect(policy.rankPolicyFallbackCandidates(trace, new Set())).toEqual([]);
  });

  it("allows another model on the same API after a model fails", async () => {
    const failed = new Response("model unavailable", { status: 502 });
    const healthy = new Response("ok");
    const config = { defaultProvider: "current" };
    const seen = [];
    const core = vi.fn(async (req) => {
      const body = await req.json();
      seen.push(body.model);
      return body.model === "current/first" ? failed : healthy;
    });
    const policy = loadPolicy(core);
    const request = (model) => new Request("http://localhost/v1/responses", {
      method: "POST", body: JSON.stringify({ model }),
    });
    expect(await policy.handleResponses(request("current/first"), config, {})).toBe(failed);
    expect(await policy.handleResponses(request("current/second"), config, {})).toBe(healthy);
    expect(seen).toEqual(["current/first", "current/second"]);
    expect(config.defaultProvider).toBe("current");
  });

  it("keeps a streaming response intact and forwards callbacks", async () => {
    const response = new Response("event: response.failed\ndata: {}\n\n", {
      headers: { "content-type": "text/event-stream" },
    });
    const onRequestBodyRead = vi.fn();
    const core = vi.fn(async (_req, _config, _log, options) => {
      options.onRequestBodyRead();
      return response;
    });
    const result = await loadPolicy(core).handleResponses({}, {}, {}, { onRequestBodyRead });
    expect(result).toBe(response);
    expect(result.bodyUsed).toBe(false);
    expect(onRequestBodyRead).toHaveBeenCalledTimes(1);
    expect(core).toHaveBeenCalledTimes(1);
  });

  it("preserves exceptions and local overload handling", async () => {
    const error = new Error("network failure");
    const core = vi.fn().mockRejectedValue(error);
    await expect(loadPolicy(core).handleResponses({}, {}, {})).rejects.toBe(error);
    const busy = new Response("busy", { status: 503 });
    expect(await loadPolicy(core, () => busy).handleResponses({}, {}, {})).toBe(busy);
  });
});
