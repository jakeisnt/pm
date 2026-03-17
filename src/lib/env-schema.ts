import { createEnv } from "@uln/env";
import { z } from "zod";

export const env = createEnv({
  SHELL: { schema: z.string(), fallback: "/bin/bash" },
  EDITOR: { schema: z.string(), fallback: "cursor" },
});
