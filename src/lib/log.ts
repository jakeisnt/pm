import pc from "picocolors";

function write(s: string): void {
  process.stderr.write(`${s}\n`);
}

export const log = {
  phase: (msg: string) => write(`  ${pc.bold(msg)}`),
  item: (msg: string) => write(`  ${msg}`),
  detail: (msg: string) => write(`    ${msg}`),
  success: (msg: string) => write(`  ${pc.green("✓")} ${msg}`),
  warn: (msg: string) => write(`  ${pc.yellow("⚠")} ${msg}`),
  fail: (msg: string) => write(`  ${pc.red("✗")} ${msg}`),
  dim: (msg: string) => write(`  ${pc.dim(msg)}`),
  blank: () => write(""),
  raw: (msg: string) => process.stdout.write(`${msg}\n`),
  info: (msg: string) => write(`  ${pc.blue("ℹ")} ${msg}`),
};
