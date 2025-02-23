import generouted from '@generouted/react-router/plugin'
import ViteYaml from '@modyfi/vite-plugin-yaml'
import react from '@vitejs/plugin-react'
import { bytecodePlugin, defineConfig, externalizeDepsPlugin } from 'electron-vite'
import { merge } from 'lodash-es'
import { resolve } from 'path'
import { AliasOptions, ResolveOptions } from 'vite'
console.log('current working dir:', resolve('.'))
const resolveOptions: { resolve: ResolveOptions & { alias: AliasOptions } } = {
  resolve: {
    alias: {
      '@renderer': resolve('src/renderer/src'),
      '@main': resolve('src/main'),
      '@preload': resolve('src/preload'),
      '@shared': resolve('src/shared'),
      '@': resolve('src'),
      '~': resolve('.')
    }
  }
}
const externalizedEsmDeps = [
  'lodash-es',
  '@faker-js/faker',
  '@trpc-limiter/memory',
  'got',
  'encryption.js',
  'filenamify',
  'yt-dlp-wrap',
  'p-queue'
]
const isMac = !!process.env.ACTION_RUNNER?.startsWith('macos-')
const isProduction = process.env.NODE_ENV === "production";
export default defineConfig({
  main: {
    ...resolveOptions,
    plugins: [
      externalizeDepsPlugin({ exclude: [...externalizedEsmDeps] }),
      ViteYaml(),
      ...((!isMac && isProduction && [bytecodePlugin({ transformArrowFunctions: false })]) || [])
    ],
    build: {
      rollupOptions: {
        output: {
          manualChunks: (id: string): any => {
            if (externalizedEsmDeps.find((d) => d === id)) return id
          }
        }
      }
    },
    publicDir: './resources'
  },
  preload: {
    ...resolveOptions,
    plugins: [
      externalizeDepsPlugin({ exclude: [...externalizedEsmDeps] }),
      ViteYaml(),
      ...((!isMac && isProduction && [bytecodePlugin({ transformArrowFunctions: false })]) || [])
    ]
  },
  renderer: {
    ...merge(resolveOptions, {
      resolve: {
        alias: {
          '@': resolve('src/renderer/src')
        }
      }
    }),
    plugins: [
      ViteYaml(),
      react({
        babel: {
          plugins: [
            [
              'styled-jsx/babel',
              {
                plugins: [
                  [
                    'styled-jsx-plugin-postcss',
                    {
                      path: './postcss.config.js',
                      compileEnv: 'worker'
                    }
                  ]
                ]
              }
            ]
          ]
        }
      }),
      generouted({
        source: {
          routes: './src/renderer/src/pages/**/{page,index}.{jsx,tsx}',
          modals: './src/renderer/src/pages/**/[+]*.{jsx,tsx}'
        },
        output: resolve('./src/renderer/src/router.ts')
      })
    ]
  }
})
