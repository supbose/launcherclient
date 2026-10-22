//! 五套启动器界面模板共用的派生逻辑
//!
//! 只做「数据 → 展示文本/状态」的换算，不含任何布局与配色；
//! 布局与配色各模板自己持有（见 UpdateTemplateA~E.vue），互不覆盖。

import { computed, unref } from 'vue';

/// 后端阶段常量（与 PhasePayload.stage 一一对应）
export const STAGE = {
  CHECKING: 'checking',
  DOWNLOADING: 'downloading',
  INSTALLING: 'installing',
  UP_TO_DATE: 'up-to-date',
  AVAILABLE: 'available',
  DONE: 'done',
  ERROR: 'error',
};

/// 步骤条节点文案（仅 C 模板使用）：下载 → 校验 → 解压 → 完成
export const STEP_LABELS = ['下载', '校验', '解压', '完成'];

/// 字节数格式化（与登录器 QsLtdLauncherClient 同款格式）
export function formatBytes(bytes) {
  const n = Number(bytes) || 0;
  if (n <= 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.min(Math.floor(Math.log(n) / Math.log(1024)), units.length - 1);
  return (n / Math.pow(1024, i)).toFixed(2) + ' ' + units[i];
}

/// 读取阶段数据：支持 ref、普通对象与 getter（模板里常用 `() => props.phase`
/// 保持响应式；unref 不解包函数，故此处显式调用一次）
const readPhase = (phase) => (typeof phase === 'function' ? phase() : unref(phase)) || {};

/// 把阶段载荷换算成模板可直接渲染的展示值
///
/// @param phase 阶段对象 / ref / getter：{stage,message,sub,percent,downloadedBytes,totalBytes,speedBps,changelog,badge,step}
export function usePhase(phase) {
  const p = computed(() => readPhase(phase));

  const stage = computed(() => p.value.stage || STAGE.CHECKING);
  const message = computed(() => p.value.message || '');
  const sub = computed(() => p.value.sub || '');
  const badge = computed(() => p.value.badge || '');

  const percentValue = computed(() => {
    const v = Number(p.value.percent);
    if (!Number.isFinite(v)) return 0;
    return Math.min(100, Math.max(0, v));
  });

  /// "12.30 MB / 45.60 MB"（总大小未知时只显示已下载）
  const sizeText = computed(() => {
    const done = Number(p.value.downloadedBytes) || 0;
    const total = Number(p.value.totalBytes) || 0;
    if (!done) return '';
    return total > 0
      ? `${formatBytes(done)} / ${formatBytes(total)}`
      : formatBytes(done);
  });

  /// "2.50 MB/s"
  const speedText = computed(() => {
    const bps = Number(p.value.speedBps) || 0;
    return bps > 0 ? `${formatBytes(bps)}/s` : '';
  });

  /// 剩余时间（按当前速度估算；速度未知或已下完时不显示）
  const etaText = computed(() => {
    const done = Number(p.value.downloadedBytes) || 0;
    const total = Number(p.value.totalBytes) || 0;
    const bps = Number(p.value.speedBps) || 0;
    const remain = total - done;
    if (bps <= 0 || remain <= 0) return '';
    const secs = Math.round(remain / bps);
    if (secs < 60) return `剩余约 ${secs} 秒`;
    const mins = Math.round(secs / 60);
    if (mins < 60) return `剩余约 ${mins} 分钟`;
    return `剩余约 ${Math.floor(mins / 60)} 小时 ${mins % 60} 分`;
  });

  /// 更新日志（过滤空行，避免渲染空条目）
  const changelog = computed(() =>
    Array.isArray(p.value.changelog)
      ? p.value.changelog.filter((s) => typeof s === 'string' && s.trim())
      : []
  );

  /// 是否显示进度条：下载/安装/合并（checking 带百分比即合并分卷）
  const showProgress = computed(
    () =>
      stage.value === STAGE.DOWNLOADING ||
      stage.value === STAGE.INSTALLING ||
      (stage.value === STAGE.CHECKING && !!sub.value)
  );

  /// 步骤条进度（-1 表示流程尚未开始）
  const step = computed(() => {
    const v = Number(p.value.step);
    return Number.isFinite(v) ? v : -1;
  });

  return {
    stage,
    message,
    sub,
    badge,
    percentValue,
    sizeText,
    speedText,
    etaText,
    changelog,
    showProgress,
    step,
  };
}
