/** Whether stdin is connected to a TTY (i.e., a human is at the terminal). */
export const isInteractive = (): boolean => Boolean(process.stdin.isTTY);
