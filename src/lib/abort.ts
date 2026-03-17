import { ESC_DISAMBIGUATE_MS, EXIT_ABORT } from "./config/index.ts";
import { isInteractive } from "./terminal.ts";

let aborted = false;
let activeProc: { kill(sig?: number | string): void } | null = null;
let suspended = false;

export function checkAbort(): void {
  if (aborted) {
    throw new AbortError();
  }
}

export class AbortError extends Error {
  constructor() {
    super("Aborted.");
    this.name = "AbortError";
  }
}

export function trackProcess(proc: { kill(sig?: number | string): void } | null) {
  activeProc = proc;
}

export function enableAbort() {
  if (!isInteractive()) return;

  let escPending = false;
  let escTimer: ReturnType<typeof setTimeout> | null = null;

  process.stdin.setRawMode(true);
  process.stdin.resume();
  process.stdin.on("data", (chunk: Buffer) => {
    if (aborted || suspended) return;

    if (escPending) {
      escPending = false;
      if (escTimer) {
        clearTimeout(escTimer);
        escTimer = null;
      }
      return;
    }

    if (chunk.length === 1 && chunk[0] === 0x1b) {
      escPending = true;
      escTimer = setTimeout(() => {
        escPending = false;
        aborted = true;
        if (activeProc) {
          try {
            activeProc.kill("SIGTERM");
          } catch {}
        }
        try {
          process.stdin.setRawMode(false);
        } catch {}
        process.stderr.write("\nAborted.\n");
        process.exit(EXIT_ABORT);
      }, ESC_DISAMBIGUATE_MS);
      return;
    }

    if (chunk.length === 1 && chunk[0] === 0x03) {
      if (activeProc) {
        try {
          activeProc.kill("SIGINT");
        } catch {}
      } else {
        try {
          process.stdin.setRawMode(false);
        } catch {}
        process.exit(EXIT_ABORT);
      }
    }
  });
}

/** Temporarily suspend abort handling (e.g. while the fuzzy selector owns the terminal). */
export function suspendAbort(): () => void {
  suspended = true;
  try {
    if (process.stdin.isTTY) {
      process.stdin.setRawMode(false);
      process.stdin.pause();
    }
  } catch {}
  return () => {
    suspended = false;
    try {
      if (process.stdin.isTTY) {
        process.stdin.setRawMode(true);
        process.stdin.resume();
      }
    } catch {}
  };
}

export function disableAbort() {
  try {
    if (process.stdin.isTTY) {
      process.stdin.setRawMode(false);
      process.stdin.pause();
    }
  } catch {}
}
