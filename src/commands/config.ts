import pc from "picocolors";
import { deleteConfigValue, getConfigPath, loadConfig, setConfigValue } from "../lib/config/index.ts";
import { log } from "../lib/log.ts";

export function runConfigList(): void {
  const config = loadConfig();
  log.blank();
  log.phase(`Config (${getConfigPath()})`);
  for (const [key, value] of Object.entries(config)) {
    log.item(`${pc.bold(key)} = ${typeof value === "string" ? value : JSON.stringify(value)}`);
  }
  log.blank();
}

export function runConfigSet(key: string, value: string): void {
  setConfigValue(key, value);
  log.success(`Set ${pc.bold(key)} = ${value}`);
}

export function runConfigDelete(key: string): void {
  deleteConfigValue(key);
  log.success(`Deleted ${pc.bold(key)} (will use default)`);
}
