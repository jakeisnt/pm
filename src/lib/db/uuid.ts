export function generateId(): string {
  return crypto.randomUUID();
}

export function shortId(uuid: string): string {
  return uuid.slice(0, 8);
}
