import { log, sym, theme } from "@uln/log";

// Route all log output to stderr so stdout stays clean for programmatic use.
// raw() still goes to stdout via the write channel — callers needing stdout
// should use process.stdout.write() directly.
log.setOutput({
  write: (msg: string) => {
    process.stderr.write(`${msg}\n`);
  },
  writeError: (msg: string) => {
    process.stderr.write(`${msg}\n`);
  },
});

export { log, sym, theme };
