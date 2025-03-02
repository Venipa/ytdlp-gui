import { pick } from 'lodash-es'
import git from '~/git.json'
import pkg from '~/package.json'
const config = {
  title: 'YTDLP GUI',
  appInfo: pick(pkg, 'name', 'author', 'version'),
  git: git as any
}
export const isProduction = import.meta.env.PROD
export const isDevelopmentOrDebug = import.meta.env.MAIN_VITE_DEBUG || !isProduction
export const NodeEnv = isProduction ? 'Production' : 'Development'
export default config
