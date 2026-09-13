/** @type {import('tailwindcss').Config} */
export default {
  content: ['./index.html', './src/**/*.{ts,tsx}'],
  darkMode: 'class',
  theme: {
    extend: {
      colors: {
        // 语义色板：UI 只用语义名，不直接写死灰度值，便于统一换肤。
        canvas: 'rgb(var(--wy-canvas) / <alpha-value>)',
        surface: 'rgb(var(--wy-surface) / <alpha-value>)',
        elevated: 'rgb(var(--wy-elevated) / <alpha-value>)',
        line: 'rgb(var(--wy-line) / <alpha-value>)',
        ink: 'rgb(var(--wy-ink) / <alpha-value>)',
        muted: 'rgb(var(--wy-muted) / <alpha-value>)',
        accent: 'rgb(var(--wy-accent) / <alpha-value>)',
        ok: 'rgb(var(--wy-ok) / <alpha-value>)',
        warn: 'rgb(var(--wy-warn) / <alpha-value>)',
        danger: 'rgb(var(--wy-danger) / <alpha-value>)',
      },
      fontFamily: {
        sans: ['Inter', 'system-ui', '-apple-system', 'PingFang SC', 'Microsoft YaHei', 'sans-serif'],
        mono: ['JetBrains Mono', 'SF Mono', 'Menlo', 'Consolas', 'monospace'],
      },
      borderRadius: { xl: '0.875rem' },
    },
  },
  plugins: [],
};
