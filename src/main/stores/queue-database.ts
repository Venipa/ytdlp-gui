import { createClient } from '@libsql/client'
import { drizzle } from 'drizzle-orm/libsql'
import { migrate } from 'drizzle-orm/libsql/migrator'
import { app } from 'electron'
import { mkdirSync } from 'fs'
import path from 'path'
import { pathToFileURL } from 'url'
import * as schema from './queue-database.schema'
// You can specify any property from the libsql connection options
const dbPath = import.meta.env.DEV ? path.join("out", 'sqlite.db') : path.join(app.getPath('userData'), 'data.db')
mkdirSync(path.dirname(dbPath), { recursive: true })
const database = createClient({ url: pathToFileURL(dbPath).toString() })
export const runMigrate = async () => {
  await migrate(db, {
    migrationsFolder: path.join(__dirname, '../../drizzle')
  })
}
export const db = drizzle(database, { schema })
