import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { resolve } from 'node:path';

// Tauri 在 dev 时通过 devUrl 加载前端，因此端口必须固定且不能被自动切换。
const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: { '@': resolve(__dirname, 'src') },
  },
  // Tauri 期望一个稳定的 dev server：固定端口、失败即报错。
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: 'ws', host, port: 1421 } : undefined,
    watch: {
      // src-tauri 由 cargo 自己监听，Vite 不要重复扫描。
      ignored: ['**/src-tauri/**'],
    },
  },
  envPrefix: ['VITE_', 'TAURI_ENV_'],
  build: {
    // macOS/Linux 走 safari 系内核，Windows 走 chromium。
    target: process.env.TAURI_ENV_PLATFORM === 'windows' ? 'chrome105' : 'safari13',
    minify: process.env.TAURI_ENV_DEBUG ? false : 'esbuild',
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
    rollupOptions: {
      output: {
        // PR-07 §37：把体积大且不常变的第三方链拆成独立 chunk。
        //
        // 诚实说明：Tauri 走本地 file:// 加载，没有网络往返，183KB gzip 在桌面端
        // **不是首屏瓶颈**（V8 解析约 10~30ms）。拆分的真实收益是——
        // (1) 每个 chunk 回到告警阈值以下，警告重新变得有意义（现在 587KB 的单块
        //     会掩盖未来真正的大块回归）；
        // (2) 稳定依赖与业务代码分离，业务改动不会让 vendor 缓存失效。
        // 没有做路由级 lazy：那是架构改动，超出本 PR 边界（任务书 §40）。
        // 用函数形式而非对象形式：对象里的模块名必须是**可解析的入口**，
        // 而 markdown 链的传递依赖（micromark / mdast / hast / unist-*）不是
        // 直接依赖，pnpm 严格 node_modules 下写了会报
        // "Could not resolve entry module"。按路径前缀匹配才能把它们一并归组。
        manualChunks(id: string) {
          if (!id.includes('node_modules')) return undefined;
          if (/[\\/]node_modules[\\/](react|react-dom|react-router|react-router-dom|zustand|scheduler)[\\/]/.test(id)) {
            return 'vendor-react';
          }
          if (/[\\/](react-markdown|remark-gfm|rehype-sanitize|rehype-|remark-|micromark|mdast|hast|unist-|parse5|property-information|space-separated-tokens|comma-separated-tokens|html-url-attributes|devlop|vfile|unist)/.test(id)) {
            return 'vendor-markdown';
          }
          if (id.includes('@radix-ui') || id.includes('class-variance-authority')) {
            return 'vendor-radix';
          }
          if (id.includes('clsx') || id.includes('tailwind-merge')) {
            return 'vendor-utils';
          }
          return undefined;
        },
      },
    },
  },
});
