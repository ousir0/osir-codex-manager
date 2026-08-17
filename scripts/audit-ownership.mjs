import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptPath = fileURLToPath(import.meta.url);
const root = path.resolve(path.dirname(scriptPath), "..");
const args = new Set(process.argv.slice(2));
const strict = args.has("--strict");
const json = args.has("--json");
const showAll = args.has("--all");

const patterns = [
  { id: "legacy-service-domain", regex: /(?:codexapp|api)\.awai\.cc/gi },
  { id: "legacy-github-owner", regex: /qq50198784[79]/g },
  { id: "legacy-history-repository", regex: /Wangnov\/codex-app-mirror/gi },
  { id: "legacy-bundle-id", regex: /cc\.awai\.codexappmanager/gi },
  { id: "legacy-brand-token", regex: /\bAWAI\b|\bawai\b|awai[-_]/g },
  { id: "legacy-data-identity", regex: /wangnov|codexappmanager/gi },
];

const textExtensions = new Set([
  "",
  ".css",
  ".html",
  ".js",
  ".json",
  ".jsx",
  ".md",
  ".mjs",
  ".nsh",
  ".nsi",
  ".plist",
  ".ps1",
  ".rs",
  ".sh",
  ".svg",
  ".toml",
  ".ts",
  ".tsx",
  ".txt",
  ".yaml",
  ".yml",
]);

function categoryFor(file) {
  if (/\.(test|spec)\.[cm]?[jt]sx?$/.test(file) || file.includes("/tests/")) {
    return "tests";
  }
  if (file.startsWith(".github/") || file.startsWith("scripts/")) {
    return "release";
  }
  if (
    file.startsWith("src/") ||
    file.startsWith("src-tauri/src/") ||
    file.startsWith("crates/") ||
    file === "src-tauri/tauri.conf.json" ||
    file === "vite.config.ts"
  ) {
    return "runtime";
  }
  if (file === "README.md" || file.startsWith("assets/") || file.startsWith("website/")) {
    return "public";
  }
  return "other";
}

function shouldScan(file) {
  if (
    file === "scripts/audit-ownership.mjs" ||
    file === "SPEC.md" ||
    file === "GOAL.md" ||
    file.startsWith("docs/")
  ) {
    return false;
  }
  return textExtensions.has(path.extname(file).toLowerCase());
}

const trackedAndUntracked = execFileSync(
  "git",
  ["ls-files", "--cached", "--others", "--exclude-standard", "-z"],
  { cwd: root },
)
  .toString("utf8")
  .split("\0")
  .filter(Boolean)
  .filter(shouldScan);

const findings = [];
for (const file of trackedAndUntracked) {
  let content;
  try {
    content = readFileSync(path.join(root, file), "utf8");
  } catch {
    continue;
  }

  const lines = content.split(/\r?\n/);
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    if (line.includes("ownership-audit: allow-legacy")) continue;
    const matched = patterns
      .filter(({ regex }) => {
        regex.lastIndex = 0;
        return regex.test(line);
      })
      .map(({ id }) => id);
    if (matched.length > 0) {
      findings.push({
        file,
        line: index + 1,
        category: categoryFor(file),
        patterns: matched,
        excerpt: line.trim().slice(0, 240),
      });
    }
  }
}

const files = new Set(findings.map(({ file }) => file));
const categories = Object.fromEntries(
  ["runtime", "release", "public", "tests", "other"].map((category) => [
    category,
    {
      findings: findings.filter((item) => item.category === category).length,
      files: new Set(
        findings.filter((item) => item.category === category).map((item) => item.file),
      ).size,
    },
  ]),
);

const report = {
  schemaVersion: 1,
  scannedFiles: trackedAndUntracked.length,
  filesWithFindings: files.size,
  findingLines: findings.length,
  categories,
  findings,
};

if (json) {
  process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
} else {
  console.log(
    `Ownership audit: ${report.filesWithFindings} files, ${report.findingLines} matching lines ` +
      `(${report.scannedFiles} text files scanned).`,
  );
  for (const [category, summary] of Object.entries(categories)) {
    console.log(`- ${category}: ${summary.files} files / ${summary.findings} lines`);
  }

  const rows = showAll ? findings : findings.slice(0, 30);
  if (rows.length > 0) {
    console.log("\nFindings:");
    for (const finding of rows) {
      console.log(
        `${finding.file}:${finding.line} [${finding.category}] ${finding.patterns.join(",")}`,
      );
    }
  }
  if (!showAll && findings.length > rows.length) {
    console.log(`\n${findings.length - rows.length} more lines omitted; rerun with --all or --json.`);
  }
}

if (strict && findings.length > 0) {
  process.exitCode = 1;
}
