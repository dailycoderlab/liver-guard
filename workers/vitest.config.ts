import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    // Detection runs on plain typed arrays, so it is testable in Node with no
    // canvas or browser environment.
    include: ["console/src/**/*.test.ts"],
    environment: "node",
  },
});
