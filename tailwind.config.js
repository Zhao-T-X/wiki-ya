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

      // PR-07 §6 Typography：知识正文必须明显大于元数据。
      // 之前全站靠 `text-[11px]` / `text-xs` 任意写死，导致「正文和元数据一样大」，
      // 整个产品看起来像内部管理后台。这里把字号刻度显式命名，组件才有语义可用。
      fontSize: {
        // Level 1：用户内容
        'reading': ['0.9375rem', { lineHeight: '1.7' }],   // 15px 正文
        'body': ['0.875rem', { lineHeight: '1.7' }],      // 14px 辅助正文
        // Level 2：次要
        'secondary': ['0.8125rem', { lineHeight: '1.6' }], // 13px
        // Level 3：元数据
        'meta': ['0.6875rem', { lineHeight: '1.45' }],    // 11px
      },

      // PR-07 §7 阅读宽度：知识正文不该和后台卡片用同一个容器宽度。
      maxWidth: {
        reading: '46rem',   // 736px —— 正文最佳行长（约 45 汉字）
        search: '56rem',    // 896px —— 搜索结果
        prose: '40rem',
      },

      // PR-07 §5：知识工作台需要的少量动效。任务书只允许「状态变化」类动画，
      // 因此这里不加弹跳/缩放/发光，只保留淡入与折叠展开。
      keyframes: {
        'accordion-down': {
          from: { height: '0' },
          to: { height: 'var(--radix-accordion-content-height)' },
        },
        'accordion-up': {
          from: { height: 'var(--radix-accordion-content-height)' },
          to: { height: '0' },
        },
        'fade-in': {
          from: { opacity: '0' },
          to: { opacity: '1' },
        },
        'fade-up': {
          from: { opacity: '0', transform: 'translateY(4px)' },
          to: { opacity: '1', transform: 'none' },
        },
      },
      animation: {
        'accordion-down': 'accordion-down 0.2s ease-out',
        'accordion-up': 'accordion-up 0.2s ease-out',
        'fade-in': 'fade-in 0.18s ease-out',
        'fade-up': 'fade-up 0.2s ease-out',
      },
    },
  },
  plugins: [require('tailwindcss-animate')],
};
