import type { Config } from 'drizzle-kit'

export default {
  schema: './src/main/stores/queue-database.schema.ts',
  out: './drizzle',
  dialect: 'sqlite'
} satisfies Config
