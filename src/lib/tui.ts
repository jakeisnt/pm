/**
 * Minimal TUI framework: alternate screen, raw mode, keyboard input, ANSI rendering.
 * Zero dependencies beyond Node builtins + picocolors.
 */

import pc from "picocolors";

// ─── Types ─────────────────────────────────────────────────────────────────

export interface TuiListItem {
  /** Left icon/indicator */
  icon: string;
  /** Icon color (picocolors function name) */
  iconColor: "green" | "cyan" | "yellow" | "red" | "dim";
  /** Main display text */
  label: string;
  /** Secondary text (right side, dimmed) */
  detail: string;
  /** Match highlight positions in the label */
  highlights: number[];
}

export interface TuiSelectResult<T> {
  item: T;
  cancelled: boolean;
}

// ─── ANSI helpers ──────────────────────────────────────────────────────────

const ESC = "\x1b";
const CSI = `${ESC}[`;
const ALTERNATE_ON = `${CSI}?1049h`;
const ALTERNATE_OFF = `${CSI}?1049l`;
const CURSOR_HIDE = `${CSI}?25l`;
const CURSOR_SHOW = `${CSI}?25h`;
const CLEAR_SCREEN = `${CSI}2J`;
const CLEAR_LINE = `${CSI}2K`;

function moveTo(row: number, col: number): string {
  return `${CSI}${row + 1};${col + 1}H`;
}

function getTermSize(): { rows: number; cols: number } {
  return {
    rows: process.stdout.rows || 24,
    cols: process.stdout.columns || 80,
  };
}

// ─── Highlight rendering ──────────────────────────────────────────────────

function renderHighlighted(text: string, highlights: number[], maxWidth: number): string {
  const highlightSet = new Set(highlights);
  let result = "";
  const len = Math.min(text.length, maxWidth);
  for (let i = 0; i < len; i++) {
    const ch = text[i] ?? "";
    if (highlightSet.has(i)) {
      result += pc.bold(pc.yellow(ch));
    } else {
      result += ch;
    }
  }
  return result;
}

function applyIconColor(text: string, color: TuiListItem["iconColor"]): string {
  switch (color) {
    case "green":
      return pc.green(text);
    case "cyan":
      return pc.cyan(text);
    case "yellow":
      return pc.yellow(text);
    case "red":
      return pc.red(text);
    case "dim":
      return pc.dim(text);
  }
}

// ─── Input parsing ─────────────────────────────────────────────────────────

function parseKey(buf: Buffer): { key: string; ctrl: boolean } {
  // Ctrl+C
  if (buf[0] === 0x03) return { key: "c", ctrl: true };
  // Ctrl+P
  if (buf[0] === 0x10) return { key: "p", ctrl: true };
  // Ctrl+N
  if (buf[0] === 0x0e) return { key: "n", ctrl: true };
  // Ctrl+U (clear input)
  if (buf[0] === 0x15) return { key: "u", ctrl: true };
  // Ctrl+W (delete word)
  if (buf[0] === 0x17) return { key: "w", ctrl: true };
  // Enter
  if (buf[0] === 0x0d || buf[0] === 0x0a) return { key: "enter", ctrl: false };
  // Escape
  if (buf[0] === 0x1b && buf.length === 1) return { key: "escape", ctrl: false };
  // Tab
  if (buf[0] === 0x09) return { key: "tab", ctrl: false };
  // Backspace
  if (buf[0] === 0x7f || buf[0] === 0x08) return { key: "backspace", ctrl: false };

  // Arrow keys
  if (buf[0] === 0x1b && buf[1] === 0x5b) {
    if (buf[2] === 0x41) return { key: "up", ctrl: false };
    if (buf[2] === 0x42) return { key: "down", ctrl: false };
    if (buf[2] === 0x43) return { key: "right", ctrl: false };
    if (buf[2] === 0x44) return { key: "left", ctrl: false };
  }

  // Regular character
  const str = buf.toString("utf8");
  if (str.length > 0 && buf[0] !== undefined && buf[0] >= 0x20) {
    return { key: str, ctrl: false };
  }

  return { key: "", ctrl: false };
}

// ─── Core TUI ──────────────────────────────────────────────────────────────

export function tuiSelect<T>(
  items: T[],
  opts: {
    toListItem: (item: T, query: string) => TuiListItem;
    filter: (items: T[], query: string) => { item: T; highlights: number[] }[];
    placeholder: string;
    emptyMessage?: string;
  },
): Promise<TuiSelectResult<T>> {
  return new Promise((resolve) => {
    let query = "";
    let selectedIndex = 0;
    let scrollOffset = 0;
    let filtered: { item: T; highlights: number[] }[] = items.map((item) => ({
      item,
      highlights: [],
    }));

    const write = (s: string) => process.stderr.write(s);

    // Enter alternate screen + raw mode
    write(ALTERNATE_ON + CURSOR_HIDE + CLEAR_SCREEN);
    const wasRaw = process.stdin.isRaw;
    process.stdin.setRawMode(true);
    process.stdin.resume();

    function cleanup() {
      write(CURSOR_SHOW + ALTERNATE_OFF);
      process.stdin.setRawMode(wasRaw ?? false);
      process.stdin.pause();
      process.stdin.removeListener("data", onData);
      process.stdout.removeListener("resize", render);
    }

    function updateFilter() {
      if (!query) {
        filtered = items.map((item) => ({ item, highlights: [] }));
      } else {
        filtered = opts.filter(items, query);
      }
      selectedIndex = 0;
      scrollOffset = 0;
    }

    function render() {
      const { rows, cols } = getTermSize();
      let out = moveTo(0, 0) + CLEAR_LINE;

      // Header: search input
      const prompt = pc.bold(pc.cyan("> "));
      const queryDisplay = query || pc.dim(opts.placeholder);
      out += `${prompt}${queryDisplay}`;

      // Count
      const countStr = pc.dim(` ${filtered.length}/${items.length}`);
      const countLen = ` ${filtered.length}/${items.length}`.length;
      if (query.length + 4 + countLen < cols) {
        out += moveTo(0, cols - countLen);
        out += countStr;
      }

      // Separator
      out += moveTo(1, 0) + CLEAR_LINE + pc.dim("─".repeat(cols));

      // Available rows for list
      const listRows = rows - 3; // header + separator + bottom status
      const totalItems = filtered.length;

      // Ensure selected is visible
      if (selectedIndex < scrollOffset) {
        scrollOffset = selectedIndex;
      }
      if (selectedIndex >= scrollOffset + listRows) {
        scrollOffset = selectedIndex - listRows + 1;
      }

      // Render visible items
      for (let i = 0; i < listRows; i++) {
        const itemIndex = scrollOffset + i;
        out += moveTo(i + 2, 0) + CLEAR_LINE;

        if (itemIndex >= totalItems) continue;

        const entry = filtered[itemIndex];
        if (!entry) continue;

        const isSelected = itemIndex === selectedIndex;
        const listItem = opts.toListItem(entry.item, query);

        // Selection indicator
        const indicator = isSelected ? pc.cyan("▸ ") : "  ";

        // Icon
        const icon = applyIconColor(listItem.icon, listItem.iconColor);

        // Label with highlights
        const maxLabelWidth = Math.max(10, cols - listItem.detail.length - 10);
        const label = renderHighlighted(listItem.label, entry.highlights, maxLabelWidth);

        // Detail (right-aligned, dimmed)
        let line = `${indicator}${icon} ${label}`;

        if (listItem.detail) {
          const detailMaxWidth = Math.max(0, cols - maxLabelWidth - 8);
          const detail =
            listItem.detail.length > detailMaxWidth
              ? `…${listItem.detail.slice(listItem.detail.length - detailMaxWidth + 1)}`
              : listItem.detail;
          line += `  ${pc.dim(detail)}`;
        }

        if (isSelected) {
          out += pc.inverse(line);
        } else {
          out += line;
        }
      }

      // Bottom status bar
      const statusRow = rows - 1;
      out += moveTo(statusRow, 0) + CLEAR_LINE;
      if (totalItems === 0) {
        out += pc.dim(opts.emptyMessage ?? "No matches");
      } else {
        out += pc.dim(`↑↓ navigate  enter select  esc cancel`);
        if (totalItems > listRows) {
          out += pc.dim(`  ${scrollOffset + 1}-${Math.min(scrollOffset + listRows, totalItems)}/${totalItems}`);
        }
      }

      write(out);
    }

    function onData(buf: Buffer) {
      const { key, ctrl } = parseKey(buf);

      if (key === "escape" || (ctrl && key === "c")) {
        cleanup();
        resolve({ item: items[0] as T, cancelled: true });
        return;
      }

      if (key === "enter") {
        const entry = filtered[selectedIndex];
        cleanup();
        if (entry) {
          resolve({ item: entry.item, cancelled: false });
        } else {
          resolve({ item: items[0] as T, cancelled: true });
        }
        return;
      }

      if (key === "up" || (ctrl && key === "p")) {
        if (selectedIndex > 0) selectedIndex--;
        render();
        return;
      }

      if (key === "down" || (ctrl && key === "n")) {
        if (selectedIndex < filtered.length - 1) selectedIndex++;
        render();
        return;
      }

      if (key === "tab") {
        if (filtered.length > 0) {
          selectedIndex = (selectedIndex + 1) % filtered.length;
        }
        render();
        return;
      }

      if (key === "backspace") {
        if (query.length > 0) {
          query = query.slice(0, -1);
          updateFilter();
        }
        render();
        return;
      }

      if (ctrl && key === "u") {
        query = "";
        updateFilter();
        render();
        return;
      }

      if (ctrl && key === "w") {
        // Delete last word
        const trimmed = query.trimEnd();
        const lastSpace = trimmed.lastIndexOf(" ");
        query = lastSpace >= 0 ? trimmed.slice(0, lastSpace + 1) : "";
        updateFilter();
        render();
        return;
      }

      // Regular character input
      if (!ctrl && key.length > 0 && key !== "left" && key !== "right") {
        query += key;
        updateFilter();
        render();
        return;
      }
    }

    process.stdin.on("data", onData);
    process.stdout.on("resize", render);

    // Initial render
    render();
  });
}
