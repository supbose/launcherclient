//! 启动器界面模板本地预览入口（仅开发用，不进生产构建）
//!
//! `npx vite` 后访问 /preview.html：切换 A/B/C/D/E 五套模板与全部阶段，
//! 用真实模板组件渲染，便于对比与截图确认。

import { createApp } from 'vue';
import './preview.css';
import Preview from './Preview.vue';

createApp(Preview).mount('#preview');
