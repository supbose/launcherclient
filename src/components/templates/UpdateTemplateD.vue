<template>
  <!-- 方案 D：极简留白 · 居中大留白，细进度条，更新日志内联无卡片 -->
  <div class="tpl-d">
    <!-- 检测 / 合并中 -->
    <template v-if="stage === 'checking'">
      <div class="tpl-d__title">{{ message || '正在检查更新...' }}</div>
      <div class="tpl-d__row"><div class="tpl-d__spin"></div><div class="tpl-d__sub">{{ sub }}</div></div>
      <div class="tpl-d__track" v-if="sub && percentValue > 0">
        <div class="tpl-d__fill" :style="{ width: percentValue + '%' }"></div>
      </div>
    </template>

    <!-- 下载中 -->
    <template v-else-if="stage === 'downloading'">
      <div class="tpl-d__title">{{ message || '正在下载更新包...' }}</div>
      <div class="tpl-d__hint" v-if="badge">{{ badge }}</div>
      <div class="tpl-d__pct">{{ percentValue.toFixed(1) }}%</div>
      <div class="tpl-d__track">
        <div class="tpl-d__fill" :style="{ width: percentValue + '%' }"></div>
      </div>
      <div class="tpl-d__sub">
        {{ [sizeText, speedText].filter(Boolean).join(' · ') }}
        <template v-if="etaText"> · {{ etaText }}</template>
      </div>
    </template>

    <!-- 安装（解压）中 -->
    <template v-else-if="stage === 'installing'">
      <div class="tpl-d__title">{{ message || '正在安装更新...' }}</div>
      <div class="tpl-d__row"><div class="tpl-d__spin tpl-d__spin--green"></div><div class="tpl-d__sub">{{ sub }}</div></div>
      <div class="tpl-d__pct">{{ percentValue.toFixed(1) }}%</div>
      <div class="tpl-d__track">
        <div class="tpl-d__fill" :style="{ width: percentValue + '%' }"></div>
      </div>
    </template>

    <!-- 结束态 -->
    <template v-else>
      <div
        class="tpl-d__dot"
        :class="{
          'tpl-d__dot--ok': stage === 'up-to-date' || stage === 'done',
          'tpl-d__dot--info': stage === 'available',
          'tpl-d__dot--err': stage === 'error',
        }"
      ></div>
      <div class="tpl-d__title" :class="{ 'tpl-d__title--err': stage === 'error' }">
        {{ message || (stage === 'up-to-date' ? '已是最新版本' : '更新完成') }}
      </div>
      <div class="tpl-d__sub" :class="{ 'tpl-d__sub--err': stage === 'error' }" v-if="sub">{{ sub }}</div>
    </template>

    <!-- 更新日志：内联，细分割线分隔，无卡片容器 -->
    <div class="tpl-d__log" v-if="changelog.length">
      <div class="tpl-d__log-title">更新日志</div>
      <div class="tpl-d__log-list">
        <div class="tpl-d__log-item" v-for="(line, i) in changelog" :key="i">{{ line }}</div>
      </div>
    </div>

    <div class="tpl-d__actions" v-if="stage === 'error'">
      <button class="tpl-d__btn tpl-d__btn--primary" :disabled="retrying" @click="$emit('retry')">
        {{ retrying ? '正在重试...' : '重试' }}
      </button>
      <button class="tpl-d__btn" @click="$emit('close')">关闭</button>
    </div>
    <div class="tpl-d__actions" v-else-if="stage === 'done'">
      <button class="tpl-d__btn tpl-d__btn--primary" @click="$emit('finish')">完成</button>
    </div>
  </div>
</template>

<script setup>
import { usePhase } from './usePhase.js';

const props = defineProps({
  phase: { type: Object, default: () => ({}) },
  retrying: { type: Boolean, default: false },
});

defineEmits(['retry', 'close', 'finish']);

const { stage, message, sub, badge, percentValue, sizeText, speedText, etaText, changelog } =
  usePhase(() => props.phase);
</script>

<style scoped>
.tpl-d {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  padding: 34px 32px 30px;
  box-sizing: border-box;
  gap: 10px;
}

.tpl-d__title {
  font-size: 15px;
  font-weight: 600;
  color: #e8eaf0;
  text-align: center;
}

.tpl-d__title--err {
  color: #ff5b66;
}

.tpl-d__hint {
  font-size: 11px;
  color: rgba(232, 234, 240, 0.45);
}

.tpl-d__row {
  display: flex;
  align-items: center;
  gap: 8px;
}

.tpl-d__spin {
  width: 16px;
  height: 16px;
  border: 2px solid rgba(255, 255, 255, 0.15);
  border-top-color: #4a9eff;
  border-radius: 50%;
  flex-shrink: 0;
  animation: tpl-d-spin 0.9s linear infinite;
}

.tpl-d__spin--green {
  border-top-color: #35c26e;
}

@keyframes tpl-d-spin {
  to { transform: rotate(360deg); }
}

.tpl-d__pct {
  font-size: 20px;
  font-weight: 600;
  color: #e8eaf0;
  font-variant-numeric: tabular-nums;
  margin-top: 16px;
}

.tpl-d__track {
  width: 100%;
  height: 4px;
  border-radius: 2px;
  background: rgba(255, 255, 255, 0.1);
  overflow: hidden;
  flex-shrink: 0;
  margin-top: 4px;
}

.tpl-d__fill {
  height: 100%;
  border-radius: 2px;
  background: #4a9eff;
  transition: width 0.3s ease;
}

.tpl-d__sub {
  font-size: 12px;
  color: rgba(232, 234, 240, 0.5);
  text-align: center;
  font-variant-numeric: tabular-nums;
  word-break: break-all;
}

.tpl-d__sub--err {
  color: rgba(255, 91, 102, 0.75);
}

.tpl-d__dot {
  width: 14px;
  height: 14px;
  border-radius: 50%;
  flex-shrink: 0;
}

.tpl-d__dot--ok {
  background: #35c26e;
  box-shadow: 0 0 10px rgba(53, 194, 110, 0.5);
}

.tpl-d__dot--info {
  background: #4a9eff;
  box-shadow: 0 0 10px rgba(74, 158, 255, 0.5);
}

.tpl-d__dot--err {
  background: #e81123;
  box-shadow: 0 0 10px rgba(232, 17, 35, 0.5);
}

.tpl-d__log {
  width: 100%;
  margin-top: 20px;
}

.tpl-d__log-title {
  font-size: 11px;
  color: rgba(232, 234, 240, 0.4);
  letter-spacing: 0.5px;
}

.tpl-d__log-list {
  margin-top: 10px;
  border-top: 0.5px solid rgba(255, 255, 255, 0.09);
  max-height: 150px;
  overflow-y: auto;
}

.tpl-d__log-item {
  padding: 11px 0;
  border-bottom: 0.5px solid rgba(255, 255, 255, 0.09);
  font-size: 12.5px;
  line-height: 1.6;
  color: rgba(232, 234, 240, 0.85);
  word-break: break-word;
}

.tpl-d__log-item:last-child {
  border-bottom: none;
}

.tpl-d__actions {
  display: flex;
  gap: 10px;
  margin-top: 18px;
}

.tpl-d__btn {
  padding: 7px 26px;
  border: 1px solid rgba(255, 255, 255, 0.2);
  border-radius: 6px;
  background: transparent;
  color: #e8eaf0;
  font-size: 13px;
  cursor: pointer;
  transition: background 0.2s, border-color 0.2s;
}

.tpl-d__btn:hover {
  background: rgba(255, 255, 255, 0.1);
}

.tpl-d__btn--primary {
  border-color: #4a9eff;
  background: #4a9eff;
  color: #fff;
}

.tpl-d__btn--primary:disabled {
  opacity: 0.6;
  cursor: default;
}
</style>
