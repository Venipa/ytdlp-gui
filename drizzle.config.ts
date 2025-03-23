import type { Config } from 'drizzle-kit'

export default {
  schema: './src/main/stores/app-database.schema.ts',
  out: './drizzle',
  dialect: 'sqlite'
} satisfies Config
