// Codex Manager: preserve the selected route, including when the upstream fails.
// Replacement for OpenCodex 2.22.0's Responses policy fallback entry point.
import { handleResponses as handleResponsesCore } from "./core";
import { requestPacingOverloadResponse } from "./pacing-overload";
import type { RouteCandidateTrace, RouteDecisionTraceV1 } from "../../routing/trace";

type CoreHandler = typeof handleResponsesCore;

export interface PolicyFallbackDeps {
  runCore?: CoreHandler;
}

export function rankPolicyFallbackCandidates(
  _trace: RouteDecisionTraceV1,
  _tried: ReadonlySet<string>,
): RouteCandidateTrace[] {
  return [];
}

export async function handleResponsesWithPolicyFallback(
  req: Parameters<CoreHandler>[0],
  config: Parameters<CoreHandler>[1],
  logCtx: Parameters<CoreHandler>[2],
  options: Parameters<CoreHandler>[3] = {},
  deps: PolicyFallbackDeps = {},
): Promise<Response> {
  const runCore = deps.runCore ?? handleResponsesCore;
  try {
    return await runCore(req, config, logCtx, options);
  } catch (error) {
    const overload = requestPacingOverloadResponse(error);
    if (overload) return overload;
    throw error;
  }
}

export const handleResponses = handleResponsesWithPolicyFallback;
