import * as readline from "node:readline";

export class SelectionCancelledError extends Error {
  constructor() {
    super("Selection cancelled");
    this.name = "SelectionCancelledError";
  }
}

async function runFzf(input: string, extraArgs: string[] = []): Promise<string> {
  const args = ["fzf", "--no-sort", "--ansi", ...extraArgs];
  const proc = Bun.spawn(args, {
    stdin: new Response(input),
    stdout: "pipe",
    stderr: "inherit",
  });

  const output = await new Response(proc.stdout).text();
  const exitCode = await proc.exited;

  if (exitCode !== 0 || !output.trim()) {
    throw new SelectionCancelledError();
  }

  return output.trim();
}

export async function fzfSelect<T>(
  items: T[],
  opts: {
    format: (item: T, index: number) => string;
    searchKey?: (item: T, index: number) => string;
    fzfArgs?: string[];
    noMatchError?: string;
  },
): Promise<T> {
  if (items.length === 0) {
    throw new Error(opts.noMatchError ?? "No items to select from");
  }
  const first = items[0];
  if (items.length === 1 && first !== undefined) {
    return first;
  }

  let input: string;
  let baseArgs: string[];

  const { searchKey } = opts;
  if (searchKey) {
    input = items.map((item, i) => `${searchKey(item, i)}\t${opts.format(item, i)}\t${i}`).join("\n");
    baseArgs = ["--with-nth=2", "--nth=1", "--delimiter=\t"];
  } else {
    input = items.map((item, i) => `${opts.format(item, i)}\t${i}`).join("\n");
    baseArgs = ["--with-nth=1", "--delimiter=\t"];
  }

  const selected = await runFzf(input, [...baseArgs, ...(opts.fzfArgs ?? [])]);

  const idxStr = selected.split("\t").pop();
  if (!idxStr) throw new Error("Could not match selection");
  const idx = Number.parseInt(idxStr, 10);
  if (Number.isNaN(idx) || idx < 0 || idx >= items.length) {
    throw new Error("Could not match selection");
  }
  const item = items[idx];
  if (!item) throw new Error("Could not match selection");
  return item;
}

function prepareStdin(): () => void {
  const wasRaw = process.stdin.isTTY && process.stdin.isRaw;
  const wasPaused = process.stdin.isPaused();

  if (wasRaw) {
    process.stdin.setRawMode(false);
  }
  if (wasPaused) {
    process.stdin.resume();
  }

  return () => {
    if (wasRaw) {
      try {
        process.stdin.setRawMode(true);
      } catch {}
    }
    if (wasPaused) {
      process.stdin.pause();
    }
  };
}

export function askLine(prompt: string): Promise<string> {
  const restoreStdin = prepareStdin();
  const rl = readline.createInterface({
    input: process.stdin,
    output: process.stderr,
    terminal: Boolean(process.stdin.isTTY),
  });
  return new Promise((resolve) => {
    rl.question(prompt, (answer) => {
      rl.close();
      restoreStdin();
      resolve(answer.trim());
    });
  });
}
