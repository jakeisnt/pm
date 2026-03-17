import * as readline from "node:readline";
import { suspendAbort } from "./abort.ts";
import { fuzzyFilter } from "./fuzzy-match.ts";

const MAX_VISIBLE = 15;

export class SelectionCancelledError extends Error {
  constructor() {
    super("Selection cancelled");
    this.name = "SelectionCancelledError";
  }
}

// ─── Inline key parsing ─────────────────────────────────────────────────────

function parseKey(buf: Buffer): { key: string; ctrl: boolean } {
  if (buf[0] === 0x03) return { key: "c", ctrl: true };
  if (buf[0] === 0x15) return { key: "u", ctrl: true };
  if (buf[0] === 0x17) return { key: "w", ctrl: true };
  if (buf[0] === 0x10) return { key: "p", ctrl: true };
  if (buf[0] === 0x0e) return { key: "n", ctrl: true };
  if (buf[0] === 0x0d || buf[0] === 0x0a) return { key: "enter", ctrl: false };
  if (buf[0] === 0x1b && buf.length === 1) return { key: "escape", ctrl: false };
  if (buf[0] === 0x09) return { key: "tab", ctrl: false };
  if (buf[0] === 0x7f || buf[0] === 0x08) return { key: "backspace", ctrl: false };

  if (buf[0] === 0x1b && buf[1] === 0x5b) {
    if (buf[2] === 0x41) return { key: "up", ctrl: false };
    if (buf[2] === 0x42) return { key: "down", ctrl: false };
  }

  const str = buf.toString("utf8");
  if (str.length > 0 && buf[0] !== undefined && buf[0] >= 0x20) {
    return { key: str, ctrl: false };
  }
  return { key: "", ctrl: false };
}

// ─── Inline fuzzy selector ──────────────────────────────────────────────────

interface InlineEntry<T> {
  item: T;
  index: number;
  display: string;
  searchText: string;
}

export async function fzfSelect<T>(
  items: T[],
  opts: {
    format: (item: T, index: number) => string;
    searchKey?: (item: T, index: number) => string;
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

  const entries: InlineEntry<T>[] = items.map((item, i) => ({
    item,
    index: i,
    display: opts.format(item, i),
    searchText: opts.searchKey ? opts.searchKey(item, i) : opts.format(item, i),
  }));

  const resumeAbort = suspendAbort();
  try {
    return await runInlineSelect(entries);
  } finally {
    resumeAbort();
  }
}

function runInlineSelect<T>(entries: InlineEntry<T>[]): Promise<T> {
  return new Promise((resolve, reject) => {
    let query = "";
    let selectedIndex = 0;
    let scrollOffset = 0;
    let renderedLines = 0;

    const write = (s: string) => process.stderr.write(s);

    // Filter entries based on query
    function getFiltered(): InlineEntry<T>[] {
      if (!query) return entries;
      const results = fuzzyFilter(entries, query, (e) => e.searchText);
      return results.map((r) => r.item);
    }

    function render() {
      const filtered = getFiltered();
      const visibleCount = Math.min(filtered.length, MAX_VISIBLE);

      // Clear previous output
      if (renderedLines > 0) {
        // Move up and clear each line
        for (let i = 0; i < renderedLines; i++) {
          write("\x1b[A\x1b[2K");
        }
      }

      // Ensure selected is in view
      if (selectedIndex < scrollOffset) {
        scrollOffset = selectedIndex;
      }
      if (selectedIndex >= scrollOffset + visibleCount) {
        scrollOffset = selectedIndex - visibleCount + 1;
      }

      // Prompt line
      const countInfo = `\x1b[2m${filtered.length}/${entries.length}\x1b[0m`;
      const promptLine = `\x1b[36m>\x1b[0m ${query}\x1b[0K  ${countInfo}`;
      write(`${promptLine}\n`);

      let lines = 1;
      // List items
      for (let i = 0; i < visibleCount; i++) {
        const idx = scrollOffset + i;
        const entry = filtered[idx];
        if (!entry) continue;
        const isSelected = idx === selectedIndex;
        const pointer = isSelected ? "\x1b[36m▸\x1b[0m " : "  ";
        const display = isSelected ? `\x1b[1m${entry.display}\x1b[0m` : entry.display;
        write(`${pointer}${display}\x1b[0K\n`);
        lines++;
      }

      if (filtered.length === 0) {
        write("\x1b[2m  No matches\x1b[0m\x1b[0K\n");
        lines++;
      }

      renderedLines = lines;
    }

    // Initial render
    const wasRaw = process.stdin.isRaw;
    process.stdin.setRawMode(true);
    process.stdin.resume();
    render();

    function cleanup() {
      process.stdin.removeListener("data", onData);
      process.stdin.setRawMode(wasRaw ?? false);
      process.stdin.pause();
    }

    function onData(buf: Buffer) {
      const { key, ctrl } = parseKey(buf);
      const filtered = getFiltered();

      if (key === "escape" || (ctrl && key === "c")) {
        cleanup();
        // Clear the selector output
        for (let i = 0; i < renderedLines; i++) {
          write("\x1b[A\x1b[2K");
        }
        reject(new SelectionCancelledError());
        return;
      }

      if (key === "enter") {
        const entry = filtered[selectedIndex];
        cleanup();
        // Clear the selector output
        for (let i = 0; i < renderedLines; i++) {
          write("\x1b[A\x1b[2K");
        }
        if (entry) {
          resolve(entry.item);
        } else {
          reject(new SelectionCancelledError());
        }
        return;
      }

      if (key === "up" || (ctrl && key === "p")) {
        if (selectedIndex > 0) selectedIndex--;
        render();
        return;
      }

      if (key === "down" || (ctrl && key === "n") || key === "tab") {
        if (selectedIndex < filtered.length - 1) selectedIndex++;
        render();
        return;
      }

      if (key === "backspace") {
        if (query.length > 0) {
          query = query.slice(0, -1);
          selectedIndex = 0;
          scrollOffset = 0;
        }
        render();
        return;
      }

      if (ctrl && key === "u") {
        query = "";
        selectedIndex = 0;
        scrollOffset = 0;
        render();
        return;
      }

      if (ctrl && key === "w") {
        const trimmed = query.trimEnd();
        const lastSpace = trimmed.lastIndexOf(" ");
        query = lastSpace >= 0 ? trimmed.slice(0, lastSpace + 1) : "";
        selectedIndex = 0;
        scrollOffset = 0;
        render();
        return;
      }

      // Regular character
      if (!ctrl && key.length > 0 && key !== "up" && key !== "down") {
        query += key;
        selectedIndex = 0;
        scrollOffset = 0;
        render();
      }
    }

    process.stdin.on("data", onData);
  });
}

// ─── Simple line prompt ─────────────────────────────────────────────────────

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
