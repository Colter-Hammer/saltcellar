// Unused files, exports and dependencies (docs/standards.md#frontend).
import type { KnipConfig } from 'knip'

export default {
  // scripts/gen-api.sh writes src/api/generated/ whole; what it exports isn't ours to trim.
  ignore: ['src/api/generated/**'],
  // Run by scripts/gen-api.sh, outside package.json's scripts.
  ignoreDependencies: ['openapi-typescript'],
} satisfies KnipConfig
