import { readFileSync } from "node:fs";
import { join } from "node:path";
import { DEFAULT_ON_ERROR, DEFAULT_ON_MISSING_COMMAND } from "./constants.ts";

interface PmConfig {
  onMissingCommand: string;
  onError: string;
}

const CONFIG_PATH = join(import.meta.dirname, "../../config.json");

let cached: PmConfig | undefined;

export const loadConfig = (): PmConfig => {
  if (cached) return cached;
  try {
    const raw = JSON.parse(readFileSync(CONFIG_PATH, "utf-8")) as Partial<PmConfig>;
    cached = {
      onMissingCommand: raw.onMissingCommand ?? DEFAULT_ON_MISSING_COMMAND,
      onError: raw.onError ?? DEFAULT_ON_ERROR,
    };
  } catch {
    cached = {
      onMissingCommand: DEFAULT_ON_MISSING_COMMAND,
      onError: DEFAULT_ON_ERROR,
    };
  }
  return cached;
};
