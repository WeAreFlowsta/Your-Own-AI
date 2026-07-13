/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
    "./src/routes/**/*.{js,ts,jsx,tsx}",
  ],
  theme: {
    extend: {
      fontFamily: {
        varela: ['"Varela Round"', 'sans-serif'],
        jakarta: ['"Plus Jakarta Sans"', 'sans-serif'],
        mono: ['"JetBrains Mono"', 'ui-monospace', 'SFMono-Regular', 'Menlo', 'Consolas', 'monospace'],
      },
      keyframes: {
        'pulse-text': {
          '0%, 100%': { opacity: '1' },
          '50%': { opacity: '0.5' },
        },
      },
      animation: {
        'pulse-text': 'pulse-text 2s cubic-bezier(0.4, 0, 0.6, 1) infinite',
      },
    },
  },
  plugins: [],
}