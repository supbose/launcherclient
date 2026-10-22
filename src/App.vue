<template>
  <div class="updater-root">
    <MinTitleBar />
    <!-- 界面模板由后端 launcher_template 配置决定（默认 C），
         本地预览可用 ?template=A|B|C|D|E 覆盖 -->
    <component
      :is="activeTemplate"
      :phase="phase"
      :retrying="retrying"
      @retry="retryUpdate"
      @close="closeApp"
      @finish="finishUpdate"
    />
  </div>
</template>

<script setup>
import { ref, computed, onMounted, onUnmounted } from 'vue';
import MinTitleBar from './components/MinTitleBar.vue';
import {
  DEFAULT_TEMPLATE,
  resolveTemplateKey,
  templateComponent,
  previewTemplateFromQuery,
} from './components/templates/index.js';

/// 当前阶段载荷（模板只读它，不自己维护状态）
const phase = ref({
  stage: 'checking',
  message: '',
  sub: '',
  percent: 0,
  downloadedBytes: 0,
  totalBytes: 0,
  speedBps: 0,
  changelog: [],
  badge: '',
  step: -1,
});

/// 生效的界面模板键（A/B/C/D/E）
const templateKey = ref(DEFAULT_TEMPLATE);

/// 地址栏预览覆盖：?template=B（仅本地预览，生产由 update.json 决定）
const previewOverride = previewTemplateFromQuery(window.location.search);

const retrying = ref(false);

/// 是否已进入过下载阶段：后端「检测中」与「校验中」同为 checking 阶段，
/// 只有下载之后的 checking 才是步骤条上的「校验」
let hasDownloaded = false;

const activeTemplate = computed(() => templateComponent(templateKey.value));

/// 阶段 → 步骤条下标（null 表示不更新，错误态保留最后到达的步骤）
const stepOfStage = (stage) => {
  switch (stage) {
    case 'available':
    case 'downloading':
      return 0;
    case 'checking':
      return hasDownloaded ? 1 : -1;
    case 'installing':
      return 2;
    case 'done':
    case 'up-to-date':
      return 3;
    default:
      return null;
  }
};

// 应用一次阶段状态（事件推送与初始查询共用）
const applyPhase = (p) => {
  const stage = p.stage || 'checking';
  if (stage === 'downloading') hasDownloaded = true;

  const step = stepOfStage(stage);
  const prev = phase.value;
  phase.value = {
    stage,
    message: p.message || '',
    sub: p.sub || '',
    percent: p.percent ?? 0,
    // 离开下载阶段清空下载详情
    downloadedBytes: stage === 'downloading' ? p.downloaded_bytes ?? 0 : 0,
    totalBytes: stage === 'downloading' ? p.total_bytes ?? 0 : 0,
    speedBps: stage === 'downloading' ? p.speed_bps ?? 0 : 0,
    badge: stage === 'downloading' ? p.badge || '' : '',
    // 更新日志：过滤空行避免渲染空条目
    changelog: Array.isArray(p.changelog)
      ? p.changelog.filter((s) => typeof s === 'string' && s.trim())
      : [],
    step: step === null ? prev.step : step,
  };

  // 界面模板：后端每个阶段事件都携带（含 current_phase 快照）；预览覆盖优先
  if (!previewOverride) {
    templateKey.value = resolveTemplateKey(p.template);
  }

  // 收到任何非 error 阶段即视为重试已被受理（后端 run_check 首行
  // emit checking），复位按钮禁用态
  if (stage !== 'error') {
    retrying.value = false;
  }
};

// 错误态「重试」：相同命令行参数重走检测/更新流程
//（后端 spawn_check 有运行中守卫，连点/IPC 重试不会双跑）
const retryUpdate = async () => {
  retrying.value = true;
  try {
    const { invoke } = await import('@tauri-apps/api/core');
    await invoke('retry_check');
  } catch (e) {
    console.error('重试失败:', e);
    retrying.value = false;
  }
};

let unlisten = null;
// 监听建立后是否已收到过事件：快照仅作「监听建立前事件全错过」的兜底，
// 若 invoke 响应晚于后续事件到达（小包秒级走完时 IPC 延迟完全可能），
// 旧快照会把已推进的界面覆盖回旧阶段
let receivedEvent = false;

onMounted(async () => {
  try {
    const { listen } = await import('@tauri-apps/api/event');
    unlisten = await listen('update-phase', (event) => {
      receivedEvent = true;
      applyPhase(event.payload || {});
    });
    // 监听建立后主动拉取一次当前阶段：
    // 本地小更新包可能秒级走完流程，早期事件在监听建立前已 emit 完毕。
    // 仅在期间未收到任何事件时应用 —— 已收到事件说明界面比快照更新，
    // 旧快照不得回滚（invoke 响应可能晚于后续事件到达）。
    const { invoke } = await import('@tauri-apps/api/core');
    const cur = await invoke('current_phase');
    if (cur && !receivedEvent) applyPhase(cur);
  } catch (e) {
    console.error('事件监听初始化失败:', e);
  }
});

onUnmounted(() => {
  if (unlisten) unlisten();
});

const closeApp = async () => {
  try {
    const { getCurrentWindow } = await import('@tauri-apps/api/window');
    await getCurrentWindow().close();
  } catch (e) {
    console.error('关闭窗口失败:', e);
  }
};

// 「完成」：先后端启动更新后的应用（command 内部 exit），再兜底关窗
const finishUpdate = async () => {
  try {
    const { invoke } = await import('@tauri-apps/api/core');
    await invoke('finish_and_launch');
  } catch (e) {
    // command 内 app.exit(0) 会中断 IPC，reject 属预期，静默即可
    console.log('finish_and_launch 返回:', e);
  }
  await closeApp();
};
</script>

<style scoped>
.updater-root {
  width: 100vw;
  height: 100vh;
  background: linear-gradient(160deg, #1e2430 0%, #171c26 100%);
  color: #e8eaf0;
  overflow: hidden;
  position: relative;
  user-select: none;
  /* 模板根节点 flex:1 撑满标题栏以下区域；标题栏是 absolute，
     故用 padding-top 给它让位，避免模板内容被压在下面 */
  display: flex;
  flex-direction: column;
  padding-top: 32px;
  box-sizing: border-box;
}
</style>
