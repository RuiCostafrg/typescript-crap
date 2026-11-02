module.exports = {
  // service.test.ts checks this against FIXTURE_TRANSFORMER to prove this config ran.
  globals: { fixtureTransformer: 'babel-jest' },
  testEnvironment: 'node',
  testMatch: ['<rootDir>/packages/service/*.test.ts'],
  transform: { '^.+\\.tsx?$': ['babel-jest', {
    babelrc: false,
    configFile: false,
    presets: ['@babel/preset-typescript'],
    plugins: ['@babel/plugin-transform-modules-commonjs'],
  }] },
  coverageProvider: 'babel',
  collectCoverageFrom: ['packages/service/src/**/*.ts', 'packages/core/src/**/*.ts'],
};
