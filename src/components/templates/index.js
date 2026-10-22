//! 启动器界面模板注册表
//!
//! 模板键 A/B/C/D/E 与后端 `launcher_template` 配置（默认 C）一一对应。
//! 新增模板时：新建 UpdateTemplateX.vue → 在 TEMPLATE_MAP / TEMPLATE_META 登记 →
//! 把 "X" 加入后端 `config::VALID_LAUNCHER_TEMPLATES`，三处同步即可。

import UpdateTemplateA from './UpdateTemplateA.vue';
import UpdateTemplateB from './UpdateTemplateB.vue';
import UpdateTemplateC from './UpdateTemplateC.vue';
import UpdateTemplateD from './UpdateTemplateD.vue';
import UpdateTemplateE from './UpdateTemplateE.vue';

/// 默认模板（与后端 defaults::DEFAULT_LAUNCHER_TEMPLATE 保持一致）
export const DEFAULT_TEMPLATE = 'C';

/// 合法模板键
export const TEMPLATE_KEYS = ['A', 'B', 'C', 'D', 'E'];

/// 模板键 → 组件
export const TEMPLATE_MAP = {
  A: UpdateTemplateA,
  B: UpdateTemplateB,
  C: UpdateTemplateC,
  D: UpdateTemplateD,
  E: UpdateTemplateE,
};

/// 模板元信息（供配置说明与调试展示）
export const TEMPLATE_META = {
  A: { key: 'A', name: '信息密度', desc: '顶对齐 · 大百分比 · 日志卡片吸底' },
  B: { key: 'B', name: '环形居中', desc: '环形进度 · 视觉重心居中 · 日志贴底' },
  C: { key: 'C', name: '顶部步骤条', desc: '四步流程条 · 顶对齐 · 日志卡片吸底' },
  D: { key: 'D', name: '极简留白', desc: '细进度条 · 大留白 · 日志内联无卡片' },
  E: { key: 'E', name: '日志为主', desc: '进度压成状态条 · 日志占主体' },
};

/// 归一化模板键：去空白 + 大写，非法值回落默认模板
export function resolveTemplateKey(value) {
  const normalized = String(value || '')
    .trim()
    .toUpperCase();
  return TEMPLATE_KEYS.includes(normalized) ? normalized : DEFAULT_TEMPLATE;
}

/// 取模板组件（未知键一律回落默认，界面不会白屏）
export function templateComponent(key) {
  return TEMPLATE_MAP[resolveTemplateKey(key)] || TEMPLATE_MAP[DEFAULT_TEMPLATE];
}

/// 从地址栏查询串读取预览覆盖值（?template=B），未指定返回 null
///
/// 仅用于本地预览五套布局；生产由 update.json 的 launcher_template 决定。
export function previewTemplateFromQuery(search) {
  try {
    const raw = new URLSearchParams(search || '').get('template');
    if (!raw) return null;
    const normalized = raw.trim().toUpperCase();
    return TEMPLATE_KEYS.includes(normalized) ? normalized : null;
  } catch {
    return null;
  }
}
