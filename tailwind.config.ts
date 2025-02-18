import svgToDataUri from 'mini-svg-data-uri'
import type { Config } from 'tailwindcss'
import tailwindAnimate from 'tailwindcss-animate'
import { fontFamily } from 'tailwindcss/defaultTheme'

// @ts-ignore
import { default as flattenColorPalette } from 'tailwindcss/lib/util/flattenColorPalette'
function addVariablesForColors({ addBase, theme }: any) {
  let allColors = flattenColorPalette(theme('colors'))
  let newVars = Object.fromEntries(Object.entries(allColors).map(([key, val]) => [`--${key}`, val]))

  addBase({
    ':root': newVars
  })
}
export default {
  darkMode: ['class'],
  content: ['src/renderer/src/**/*.{ts,tsx}'],
  corePlugins: {
    outlineWidth: false,
    outlineColor: false,
    outlineStyle: false
  },
  theme: {
    container: {
      center: true,
      padding: '2rem',
      screens: {
        '2xl': '1400px'
      }
    },
    extend: {
      colors: {
        border: 'hsl(var(--border))',
        input: 'hsl(var(--input))',
        ring: 'hsl(var(--ring))',
        background: 'hsl(var(--background))',
        "background-2": 'hsl(var(--background-2))',
        foreground: 'hsl(var(--foreground))',
        primary: {
          DEFAULT: 'hsl(var(--primary))',
          foreground: 'hsl(var(--primary-foreground))'
        },
        secondary: {
          DEFAULT: 'hsl(var(--secondary))',
          foreground: 'hsl(var(--secondary-foreground))'
        },
        destructive: {
          DEFAULT: 'hsl(var(--destructive))',
          foreground: 'hsl(var(--destructive-foreground))'
        },
        muted: {
          DEFAULT: 'hsl(var(--muted))',
          foreground: 'hsl(var(--muted-foreground))'
        },
        accent: {
          DEFAULT: 'hsl(var(--accent))',
          foreground: 'hsl(var(--accent-foreground))'
        },
        popover: {
          DEFAULT: 'hsl(var(--popover))',
          foreground: 'hsl(var(--popover-foreground))'
        },
        card: {
          DEFAULT: 'hsl(var(--card))',
          foreground: 'hsl(var(--card-foreground))'
        }
      },
      borderRadius: {
        lg: `var(--radius)`,
        md: `calc(var(--radius) - 2px)`,
        sm: 'calc(var(--radius) - 4px)'
      },
      fontFamily: {
        sans: ['var(--font-sans)', ...fontFamily.sans]
      },
      keyframes: {
        'accordion-down': {
          from: { height: '0' },
          to: { height: 'var(--radix-accordion-content-height)' }
        },
        'accordion-up': {
          from: { height: 'var(--radix-accordion-content-height)' },
          to: { height: '0' }
        }
      },
      animation: {
        'accordion-down': 'accordion-down 0.2s ease-out',
        'accordion-up': 'accordion-up 0.2s ease-out',
        moveUp: 'moveUp 1.4s ease forwards',
        appear: 'appear 1s 1s forwards',
        marquee: 'marquee var(--duration, 30s) linear infinite'
      }
    }
  },
  plugins: [
    tailwindAnimate,
    addVariablesForColors,
    function ({ matchUtilities, theme, addUtilities }: any) {
      matchUtilities(
        {
          'bg-grid': (value: any) => ({
            backgroundImage: `url("${svgToDataUri(
              `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32" width="32" height="32" fill="none" stroke="${value}"><path d="M0 .5H31.5V32"/></svg>`
            )}")`
          }),
          'bg-grid-small': (value: any) => ({
            backgroundImage: `url("${svgToDataUri(
              `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32" width="8" height="8" fill="none" stroke="${value}"><path d="M0 .5H31.5V32"/></svg>`
            )}")`
          }),
          'bg-dot': (value: any) => ({
            backgroundImage: `url("${svgToDataUri(
              `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32" width="16" height="16" fill="none"><circle fill="${value}" id="pattern-circle" cx="10" cy="10" r="1.6257413380501518"></circle></svg>`
            )}")`
          }),
          'bg-dot-thick': (value: any) => ({
            backgroundImage: `url("${svgToDataUri(
              `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32" width="16" height="16" fill="none"><circle fill="${value}" id="pattern-circle" cx="10" cy="10" r="2.5"></circle></svg>`
            )}")`
          })
        },
        { values: flattenColorPalette(theme('backgroundColor')), type: 'color' }
      )
    },
    function ({ addUtilities, theme }) {
      const newUtilities = {
        '.vignette-clip': {
          '--vignette-color': 'black',
          '--vignette-width': '100%',
          '--vignette-height': '100%',
          maskImage:
            'radial-gradient(ellipse var(--vignette-width) var(--vignette-height) at center, var(--vignette-color) 50%, transparent 100%)',
          WebkitMaskImage:
            'radial-gradient(ellipse var(--vignette-width) var(--vignette-height) at center, var(--vignette-color) 50%, transparent 100%)'
        }
      }

      // Generate utilities for vignette colors
      const vignetteColors = theme('colors')
      Object.keys(vignetteColors).forEach((color) => {
        const colorValue = vignetteColors[color]
        if (typeof colorValue === 'string') {
          newUtilities[`.vignette-${color}`] = {
            '--vignette-color': colorValue
          }
        } else if (typeof colorValue === 'object') {
          Object.keys(colorValue).forEach((shade) => {
            newUtilities[`.vignette-${color}-${shade}`] = {
              '--vignette-color': colorValue[shade]
            }
          })
        }
      })

      // Generate utilities for vignette width and height
      const percentages = Array.from({ length: 21 }, (_, i) => i * 5)
      percentages.forEach((percent) => {
        newUtilities[`.vignette-w-${percent}`] = {
          '--vignette-width': `${percent}%`
        }
        newUtilities[`.vignette-h-${percent}`] = {
          '--vignette-height': `${percent}%`
        }
      })

      addUtilities(newUtilities)
    }
  ]
} satisfies Config
