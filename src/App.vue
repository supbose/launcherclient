<template>
  <div class="updater-root">
    <MinTitleBar />
    <div class="updater-body">
      <!-- 检查/合并中 -->
      <template v-if="stage === 'checking'">
        <div class="spinner"></div>
        <div class="status-text">{{ message || '正在检查更新...' }}</div>
        <div class="status-sub" v-if="subText">{{ subText }}</div>
        <!-- 合并阶段带进度条 -->
        <div class="progress-track" v-if="subText && progressPercent > 0">
          <div class="progress-fill" :style="{ width: progressPercent + '%' }"></div>
        </div>
      </template>

      <!-- 下载中 -->
      <template v-else-if="stage === 'downloading'">
        <div class="status-text">{{ message }}</div>
        <div class="progress-track">
          <div class="progress-fill" :style="{ width: progressPercent + '%' }"></div>
        </div>
        <div class="progress-label">
          {{ progressPercent.toFixed(1) }}%
          <template v-if="sizeText"> · {{ sizeText }}</template>
          <template v-if="speedText"> · {{ speedText }}</template>
        </div>
      </template>

      <!-- 安装中 -->
      <template v-else-if="stage === 'installing'">
        <div class="spinner"></div>
        <div class="status-text">{{ message }}</div>
        <div class="status-sub">{{ subText }}</div>
        <!-- 解压进度条 -->
        <div class="progress-track" v-if="progressPercent > 0">
          <div class="progress-fill" :style="{ width: progressPercent + '%' }"></div>
        </div>
      </template>

      <!-- 已是最新 -->
      <template v-else-if="stage === 'up-to-date'">
        <div class="status-dot ok"></div>
        <div class="status-text">已是最新版本</div>
        <div class="status-sub">{{ subText }}</div>
      </template>

      <!-- 发现新版本 -->
      <template v-else-if="stage === 'available'">
        <div class="status-dot info"></div>
        <div class="status-text">发现新版本</div>
        <div class="status-sub">{{ subText }}</div>
      </template>

      <!-- 更新完成 -->
      <template v-else-if="stage === 'done'">
        <div class="status-dot ok"></div>
        <div class="status-text">{{ message }}</div>
        <div class="status-sub">{{ subText }}</div>
        <button class="btn-close" @click="finishUpdate">完成</button>
      </template>

      <!-- 错误 -->
      <template v-else-if="stage === 'error'">
        <div class="status-dot err"></div>
        <div class="status-text error">{{ message }}</div>
        <div class="status-sub error">{{ subText }}</div>
        <button class="btn-close" @click="closeApp">关闭</button>
      </template>
    </div>
  </div>
</template>

<script setup>
import { ref, computed, onMounted, onUnmounted } from 'vue';
import MinTitleBar from './components/MinTitleBar.vue';

const stage = ref('checking');
const message = ref('');
const subText = ref('');
const percent = ref(0);
// 下载详情（后端仅下载阶段携带）
const downloadedBytes = ref(0);
const totalBytes = ref(0);
const speedBps = ref(0);

const progressPercent = computed(() => Math.min(100, Math.max(0, percent.value)));

// 字节数格式化（与登录器 QsLtdLauncherClient 同款格式）
const formatBytes = (bytes) => {
  if (!bytes || bytes <= 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  return (bytes / Math.pow(1024, i)).toFixed(2) + ' ' + units[i];
};

// "12.30 MB / 45.60 MB"（总大小未知时不显示）
const sizeText = computed(() => {
  if (!downloadedBytes.value) return '';
  return totalBytes.value > 0
    ? formatBytes(downloadedBytes.value) + ' / ' + formatBytes(totalBytes.value)
    : formatBytes(downloadedBytes.value);
});

// "2.50 MB/s"
const speedText = computed(() => {
  if (!speedBps.value) return '';
  return formatBytes(speedBps.value) + '/s';
});

let unlisten = null;

// 应用一次阶段状态（事件推送与初始查询共用）
const applyPhase = (p) => {
  stage.value = p.stage || 'checking';
  message.value = p.message || '';
  subText.value = p.sub || '';
  // 后端携带字节数时用字节计算精确百分比，未携带时保留后端百分比
  if (typeof p.downloaded_bytes === 'number' && typeof p.total_bytes === 'number' && p.total_bytes > 0) {
    percent.value = (p.downloaded_bytes / p.total_bytes) * 100;
    downloadedBytes.value = p.downloaded_bytes;
    totalBytes.value = p.total_bytes;
    speedBps.value = typeof p.speed_bps === 'number' ? p.speed_bps : 0;
  } else if (typeof p.percent === 'number') {
    percent.value = p.percent;
  }
  // 离开下载阶段时清空下载详情
  if (p.stage !== 'downloading') {
    downloadedBytes.value = 0;
    totalBytes.value = 0;
    speedBps.value = 0;
  }
};

onMounted(async () => {
  try {
    const { listen } = await import('@tauri-apps/api/event');
    unlisten = await listen('update-phase', (event) => {
      applyPhase(event.payload || {});
    });
    // 监听建立后主动拉取一次当前阶段：
    // 本地小更新包可能秒级走完全流程，早期事件在监听建立前已 emit 完毕
    const { invoke } = await import('@tauri-apps/api/core');
    const cur = await invoke('current_phase');
    if (cur) applyPhase(cur);
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
}

.updater-body {
  height: calc(100% - 32px);
  margin-top: 32px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 14px;
  padding: 0 28px;
}

.spinner {
  width: 34px;
  height: 34px;
  border: 3px solid rgba(255, 255, 255, 0.15);
  border-top-color: #4a9eff;
  border-radius: 50%;
  animation: spin 0.9s linear infinite;
}

@keyframes spin {
  to { transform: rotate(360deg); }
}

.status-dot {
  width: 14px;
  height: 14px;
  border-radius: 50%;
}

.status-dot.ok { background: #35c26e; box-shadow: 0 0 10px rgba(53, 194, 110, 0.5); }
.status-dot.info { background: #4a9eff; box-shadow: 0 0 10px rgba(74, 158, 255, 0.5); }
.status-dot.err { background: #e81123; box-shadow: 0 0 10px rgba(232, 17, 35, 0.5); }

.status-text {
  font-size: 16px;
  font-weight: 600;
}

.status-text.error {
  color: #ff5b66;
}

.status-sub {
  font-size: 12px;
  color: rgba(232, 234, 240, 0.55);
  text-align: center;
  word-break: break-all;
}

.status-sub.error {
  color: rgba(255, 91, 102, 0.75);
}

.progress-track {
  width: 100%;
  height: 8px;
  border-radius: 4px;
  background: rgba(255, 255, 255, 0.1);
  overflow: hidden;
}

.progress-fill {
  height: 100%;
  border-radius: 4px;
  background: linear-gradient(90deg, #4a9eff 0%, #35c26e 100%);
  transition: width 0.3s ease;
}

.progress-label {
  font-size: 12px;
  color: rgba(232, 234, 240, 0.65);
}

.btn-close {
  margin-top: 8px;
  padding: 7px 26px;
  border: 1px solid rgba(255, 255, 255, 0.2);
  border-radius: 6px;
  background: transparent;
  color: #e8eaf0;
  font-size: 13px;
  cursor: pointer;
  transition: background 0.2s, border-color 0.2s;
}

.btn-close:hover {
  background: rgba(255, 255, 255, 0.1);
  border-color: rgba(255, 255, 255, 0.35);
}
</style>
