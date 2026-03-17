import pc from "picocolors";
import { deleteSetting, getAllSettingsRaw, setSetting } from "../../lib/db/index.ts";
import { log } from "../../lib/log.ts";
import { SETTING_DEFS } from "../../lib/settings.ts";

export function runConfigList(): void {
  const raw = getAllSettingsRaw();

  if (raw.length === 0) {
    log.dim("No settings configured. Using defaults.");
    log.blank();
    log.phase("Available settings:");
    for (const [key, def] of Object.entries(SETTING_DEFS)) {
      log.item(`${pc.bold(key)}: ${def.description} ${pc.dim(`(default: ${def.default})`)}`);
    }
    return;
  }

  log.blank();
  log.phase("Current Settings");
  for (const { key, value } of raw) {
    log.item(`${pc.bold(key)} = ${value}`);
  }
  log.blank();
}

export function runConfigSet(key: string, value: string, opts?: { device?: boolean }): void {
  const def = SETTING_DEFS[key];
  const device = opts?.device ?? def?.deviceLocal ?? false;
  setSetting(key, value, device);
  log.success(`Set ${pc.bold(key)} = ${value}${device ? " (device-scoped)" : ""}`);
}

export function runConfigDelete(key: string): void {
  deleteSetting(key);
  log.success(`Deleted ${pc.bold(key)}`);
}
