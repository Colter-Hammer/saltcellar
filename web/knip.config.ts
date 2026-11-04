// Unused files, exports and dependencies (docs/standards.md#frontend).
import type { KnipConfig } from 'knip'

export default {
  // scripts/gen-api.sh writes src/api/generated/ whole; what it exports isn't ours to trim.
  ignore: ['src/api/generated/**'],
  // Run by scripts/gen-api.sh, outside package.json's scripts.
  // Run by `just icons`, outside package.json's scripts.
  entry: ['scripts/icons.ts'],
  ignoreDependencies: ['openapi-typescript'],
} satisfies KnipConfig
