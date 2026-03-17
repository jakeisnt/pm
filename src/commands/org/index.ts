import pc from "picocolors";
import { getOrgs, setOrgHidden } from "../../lib/db/index.ts";
import { log } from "../../lib/log.ts";

export async function runOrgList(): Promise<void> {
  const orgs = await getOrgs();
  if (orgs.length === 0) {
    log.dim("No tracked orgs.");
    return;
  }

  log.blank();
  log.phase(`Orgs (${orgs.length})`);
  log.blank();
  for (const org of orgs) {
    const icon = org.hidden ? pc.red("●") : pc.green("●");
    const label = org.hidden ? pc.dim(org.name) : pc.cyan(org.name);
    const tag = org.hidden ? pc.dim(" (hidden)") : "";
    log.item(`${icon} ${label}${tag}`);
  }
  log.blank();
}

export async function runOrgHide(name: string): Promise<void> {
  await setOrgHidden(name, true);
  log.success(`Org "${name}" is now hidden.`);
}

export async function runOrgShow(name: string): Promise<void> {
  await setOrgHidden(name, false);
  log.success(`Org "${name}" is now visible.`);
}
