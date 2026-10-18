<template>
  <div class="title-bar">
    <div class="title-bar-left" />
    <div class="title-bar-right">
      <div class="title-btn" @mousedown.stop.prevent="minimize">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <line x1="5" y1="12" x2="19" y2="12"></line>
        </svg>
      </div>
      <div class="title-btn close-btn" @mousedown.stop.prevent="close">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <line x1="18" y1="6" x2="6" y2="18"></line>
          <line x1="6" y1="6" x2="18" y2="18"></line>
        </svg>
      </div>
    </div>
  </div>
</template>

<script setup>
import { onMounted } from 'vue';

let appWindow = null;

onMounted(async () => {
  try {
    const { getCurrentWindow } = await import('@tauri-apps/api/window');
    appWindow = getCurrentWindow();
  } catch (e) {
    console.log('非 Tauri 环境');
  }
});

const minimize = async () => {
  if (!appWindow) return;
  try {
    await appWindow.minimize();
  } catch (e) {
    console.error('最小化失败:', e);
  }
};

const close = async () => {
  if (!appWindow) return;
  try {
    await appWindow.close();
  } catch (e) {
    console.error('关闭失败:', e);
  }
};
</script>

<style scoped>
.title-bar {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  height: 32px;
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 0 4px 0 12px;
  z-index: 9999;
  -webkit-app-region: drag;
}

.title-bar-right {
  display: flex;
  align-items: stretch;
  height: 100%;
  -webkit-app-region: no-drag;
}

.title-btn {
  width: 46px;
  height: 32px;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  color: rgba(255, 255, 255, 0.7);
  transition: background 0.2s, color 0.2s;
  pointer-events: auto;
}

.title-btn:hover {
  color: #ffffff;
  background: rgba(255, 255, 255, 0.1);
}

.close-btn:hover {
  background: #e81123 !important;
  color: #ffffff !important;
}

.title-bar-left {
  flex: 1;
  min-width: 0;
}
</style>
