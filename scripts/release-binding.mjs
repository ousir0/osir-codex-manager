#!/usr/bin/env node

import { createHash } from "node:crypto";
import {
  appendFileSync,
  readFileSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { basename } from "node:path";
import { isDeepStrictEqual } from "node:util";
import { fileURLToPath } from "node:url";

export const RELEASE_BINDING_PREDICATE_TYPE =
  "https://app.osirclaw.com/attestations/release-binding/v1";
export const RELEASE_WORKFLOW_PATH = ".github/workflows/release.yml";

const RELEASE_TAG_PATTERN =
  /^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/;
const REPOSITORY_PATTERN = /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/;
const SHA_PATTERN = /^[0-9a-f]{40}$/;

function assertInputs({ defaultBranch, releaseSourceSha, releaseTag, repository }) {
  if (!RELEASE_TAG_PATTERN.test(releaseTag)) {
    throw new Error("release binding target tag must be semantic vX.Y.Z");
  }
  if (!SHA_PATTERN.test(releaseSourceSha)) {
    throw new Error("release binding source must be a lowercase 40-character SHA");
  }
  if (!REPOSITORY_PATTERN.test(repository)) {
    throw new Error("release binding repository must be owner/repository");
  }
  if (
    !defaultBranch ||
    !/^[A-Za-z0-9._/-]+$/.test(defaultBranch) ||
    defaultBranch.includes("..") ||
    defaultBranch.startsWith("/") ||
    defaultBranch.endsWith("/")
  ) {
    throw new Error("release binding default branch is invalid");
  }
}

function digestFile(path) {
  const bytes = readFileSync(path);
  if (bytes.length === 0) {
    throw new Error(`release binding subject is empty: ${basename(path)}`);
  }
  return `sha256:${createHash("sha256").update(bytes).digest("hex")}`;
}

export function collectReleaseSubjectDigests(paths) {
  const entries = [];
  const names = new Set();
  for (const path of paths) {
    if (!statSync(path).isFile()) {
      throw new Error(`release binding subject is not a file: ${path}`);
    }
    const name = basename(path);
    if (name === "release-binding.json") {
      throw new Error("release-binding.json cannot recursively bind itself");
    }
    if (names.has(name)) {
      throw new Error(`release binding has duplicate subject name: ${name}`);
    }
    names.add(name);
    entries.push([name, digestFile(path)]);
  }
  if (entries.length === 0) {
    throw new Error("release binding requires at least one subject");
  }
  entries.sort(([left], [right]) => left.localeCompare(right));
  return Object.fromEntries(entries);
}

export function createReleaseBinding({
  defaultBranch,
  paths,
  releaseSourceSha,
  releaseTag,
  repository,
  trustedWorkflowSignerSha,
  trustedWorkflowSourceSha,
}) {
  assertInputs({ defaultBranch, releaseSourceSha, releaseTag, repository });
  if (
    !SHA_PATTERN.test(trustedWorkflowSignerSha) ||
    !SHA_PATTERN.test(trustedWorkflowSourceSha)
  ) {
    throw new Error(
      "trusted release workflow signer and source must be lowercase 40-character SHAs",
    );
  }
  return {
    schemaVersion: 1,
    repository,
    targetTag: releaseTag,
    releaseSourceSha,
    trustedWorkflow: {
      path: RELEASE_WORKFLOW_PATH,
      ref: `refs/heads/${defaultBranch}`,
      signerSha: trustedWorkflowSignerSha,
      sourceSha: trustedWorkflowSourceSha,
    },
    subjectDigests: collectReleaseSubjectDigests(paths),
  };
}

export function verifyReleaseBinding(
  binding,
  { defaultBranch, paths, releaseSourceSha, releaseTag, repository },
) {
  assertInputs({ defaultBranch, releaseSourceSha, releaseTag, repository });
  if (!binding || typeof binding !== "object" || Array.isArray(binding)) {
    throw new Error("release binding must be a JSON object");
  }
  if (binding.schemaVersion !== 1) {
    throw new Error("release binding schemaVersion must be 1");
  }
  if (
    binding.repository !== repository ||
    binding.targetTag !== releaseTag ||
    binding.releaseSourceSha !== releaseSourceSha
  ) {
    throw new Error("release binding does not match repository, tag, and source SHA");
  }
  const workflow = binding.trustedWorkflow;
  if (
    workflow?.path !== RELEASE_WORKFLOW_PATH ||
    workflow?.ref !== `refs/heads/${defaultBranch}` ||
    !SHA_PATTERN.test(workflow?.signerSha || "") ||
    !SHA_PATTERN.test(workflow?.sourceSha || "")
  ) {
    throw new Error("release binding does not identify a trusted default-branch workflow");
  }
  const actualDigests = collectReleaseSubjectDigests(paths);
  if (!isDeepStrictEqual(binding.subjectDigests, actualDigests)) {
    throw new Error("release binding subject digests do not match canonical assets");
  }
  return {
    signerSha: workflow.signerSha,
    sourceSha: workflow.sourceSha,
    subjectDigests: actualDigests,
  };
}

export function assertReleaseBindingAttestation(
  verificationResults,
  binding,
) {
  if (!Array.isArray(verificationResults)) {
    throw new Error("attestation verification output must be an array");
  }
  const expectedSubjects = binding?.subjectDigests;
  if (
    !expectedSubjects ||
    typeof expectedSubjects !== "object" ||
    Array.isArray(expectedSubjects)
  ) {
    throw new Error("release binding subjectDigests must be an object");
  }
  const matched = verificationResults.some((entry) => {
    const statement = entry?.verificationResult?.statement;
    if (
      statement?.predicateType !== RELEASE_BINDING_PREDICATE_TYPE ||
      !isDeepStrictEqual(statement?.predicate, binding) ||
      !Array.isArray(statement?.subject)
    ) {
      return false;
    }
    const subjectEntries = [];
    const subjectNames = new Set();
    for (const subject of statement.subject) {
      const name = subject?.name;
      const digest = subject?.digest;
      if (
        typeof name !== "string" ||
        !name ||
        subjectNames.has(name) ||
        !digest ||
        Object.keys(digest).length !== 1 ||
        !/^[0-9a-f]{64}$/.test(digest.sha256 || "")
      ) {
        return false;
      }
      subjectNames.add(name);
      subjectEntries.push([name, `sha256:${digest.sha256}`]);
    }
    subjectEntries.sort(([left], [right]) => left.localeCompare(right));
    const subjects = Object.fromEntries(subjectEntries);
    return isDeepStrictEqual(subjects, expectedSubjects);
  });
  if (!matched) {
    throw new Error(
      "verified attestation does not contain the exact release binding and subject set",
    );
  }
  return { matched: true };
}

function readJson(path, label) {
  try {
    return JSON.parse(readFileSync(path, "utf8"));
  } catch (error) {
    throw new Error(`${label} is not valid JSON: ${error.message}`);
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [, , command, ...args] = process.argv;
  try {
    if (command === "create") {
      const [
        output,
        releaseTag,
        releaseSourceSha,
        repository,
        defaultBranch,
        trustedWorkflowSignerSha,
        trustedWorkflowSourceSha,
        ...paths
      ] = args;
      const binding = createReleaseBinding({
        defaultBranch,
        paths,
        releaseSourceSha,
        releaseTag,
        repository,
        trustedWorkflowSignerSha,
        trustedWorkflowSourceSha,
      });
      writeFileSync(output, `${JSON.stringify(binding, null, 2)}\n`);
      console.log(`Created release binding for ${releaseTag} at ${releaseSourceSha}`);
    } else if (command === "verify") {
      const [bindingPath, releaseTag, releaseSourceSha, repository, defaultBranch, ...paths] = args;
      const result = verifyReleaseBinding(readJson(bindingPath, "release binding"), {
        defaultBranch,
        paths,
        releaseSourceSha,
        releaseTag,
        repository,
      });
      if (process.env.GITHUB_OUTPUT) {
        appendFileSync(process.env.GITHUB_OUTPUT, `signer_sha=${result.signerSha}\n`);
        appendFileSync(process.env.GITHUB_OUTPUT, `source_sha=${result.sourceSha}\n`);
      }
      console.log(`Verified release binding signed by workflow ${result.signerSha}`);
    } else if (command === "attestation") {
      const [verificationPath, bindingPath] = args;
      assertReleaseBindingAttestation(
        readJson(verificationPath, "attestation verification output"),
        readJson(bindingPath, "release binding"),
      );
      console.log("Verified exact release-binding attestation predicate");
    } else {
      throw new Error(
        "usage: release-binding.mjs <create|verify|attestation> ...",
      );
    }
  } catch (error) {
    console.error(`::error::${error instanceof Error ? error.message : String(error)}`);
    process.exitCode = 1;
  }
}
