const { transform } = require("./jest.config.cjs");

module.exports = {
  rootDir: __dirname,
  coverageProvider: "babel",
  collectCoverageFrom: ["packages/core/src/**/*.ts", "packages/service/src/**/*.ts"],
  coverageThreshold: {
    global: { branches: 100, functions: 100, lines: 100, statements: 100 },
  },
  projects: [
    {
      displayName: "core",
      testMatch: ["<rootDir>/jest-shards/core.test.ts"],
      transform,
    },
    {
      displayName: "service",
      testMatch: ["<rootDir>/jest-shards/service.test.ts"],
      transform,
    },
  ],
};
