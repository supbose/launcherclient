<template>
  <div class="updater-root">
    <MinTitleBar />
    <div class="updater-body">
      <!-- 检查中 -->
      <template v-if="stage === 'checking'">
        <div class="spinner"></div>
        <div class="status-text">{{ message || '正在检查更新...' }}</div>
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

      <!-- 错误 -->
      <template v-else-if="stage === 'error'">
        <div class="status-dot err"></div>
        <div class="status-text error">更新检查失败</div>
        <div class="status-sub error">{{ message }}</div>
        <button class="btn-close" @click="closeApp">关闭</button>
      </template>
    </div>
  </div>
</template>

<script setup>
import { ref, onMounted, onUnmounted } from 'vue';
import MinTitleBar from './components/MinTitleBar.vue';

const stage = ref('checking');
const message = ref('');
const subText = ref('');

let unlisten = null;

onMounted(async () => {
  try {
    const { listen } = await import('@tauri-apps/api/event');
    unlisten = await listen('update-phase', (event) => {
      const p = event.payload || {};
      stage.value = p.stage || 'checking';
      message.value = p.message || '';
      subText.value = p.sub || '';
    });
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
  padding: 0 24px;
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
