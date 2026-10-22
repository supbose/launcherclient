<template>
  <!-- 方案 A：信息密度 · 顶对齐，更新日志卡片吸底占满剩余高度 -->
  <div class="tpl-a">
    <!-- 检测 / 合并中 -->
    <template v-if="stage === 'checking'">
      <div class="tpl-a__head">
        <div class="tpl-a__spin"></div>
        <div class="tpl-a__title">{{ message || '正在检查更新...' }}</div>
      </div>
      <div class="tpl-a__sub" v-if="sub">{{ sub }}</div>
      <div class="tpl-a__track" v-if="sub && percentValue > 0">
        <div class="tpl-a__fill" :style="{ width: percentValue + '%' }"></div>
      </div>
    </template>

    <!-- 下载中 -->
    <template v-else-if="stage === 'downloading'">
      <div class="tpl-a__head tpl-a__head--split">
        <div class="tpl-a__title">{{ message || '正在下载更新包...' }}</div>
        <div class="tpl-a__pill" v-if="badge">{{ badge }}</div>
      </div>
      <div class="tpl-a__metrics">
        <div class="tpl-a__pct">{{ percentValue.toFixed(1) }}<span class="tpl-a__pct-u">%</span></div>
        <div class="tpl-a__speed">{{ speedText }}</div>
      </div>
      <div class="tpl-a__track">
        <div class="tpl-a__fill" :style="{ width: percentValue + '%' }"></div>
      </div>
      <div class="tpl-a__meta">
        <span>{{ sizeText }}</span><span>{{ etaText }}</span>
      </div>
    </template>

    <!-- 安装（解压）中 -->
    <template v-else-if="stage === 'installing'">
      <div class="tpl-a__head">
        <div class="tpl-a__spin tpl-a__spin--green"></div>
        <div class="tpl-a__title">{{ message || '正在安装更新...' }}</div>
      </div>
      <div class="tpl-a__sub" v-if="sub">{{ sub }}</div>
      <div class="tpl-a__track">
        <div class="tpl-a__fill" :style="{ width: percentValue + '%' }"></div>
      </div>
      <div class="tpl-a__meta">
        <span>{{ percentValue.toFixed(1) }}%</span><span>请勿关闭窗口</span>
      </div>
    </template>

    <!-- 已是最新 -->
    <template v-else-if="stage === 'up-to-date'">
      <div class="tpl-a__head">
        <div class="tpl-a__dot tpl-a__dot--ok"></div>
        <div class="tpl-a__title">{{ message || '已是最新版本' }}</div>
      </div>
      <div class="tpl-a__sub" v-if="sub">{{ sub }}</div>
    </template>

    <!-- 发现新版本 -->
    <template v-else-if="stage === 'available'">
      <div class="tpl-a__head">
        <div class="tpl-a__dot tpl-a__dot--info"></div>
        <div class="tpl-a__title">{{ message || '发现新版本' }}</div>
      </div>
      <div class="tpl-a__sub" v-if="sub">{{ sub }}</div>
    </template>

    <!-- 更新完成 -->
    <template v-else-if="stage === 'done'">
      <div class="tpl-a__head">
        <div class="tpl-a__dot tpl-a__dot--ok"></div>
        <div class="tpl-a__title">{{ message || '更新完成' }}</div>
      </div>
      <div class="tpl-a__sub" v-if="sub">{{ sub }}</div>
    </template>

    <!-- 错误 -->
    <template v-else>
      <div class="tpl-a__head">
        <div class="tpl-a__dot tpl-a__dot--err"></div>
        <div class="tpl-a__title tpl-a__title--err">{{ message || '更新失败' }}</div>
      </div>
      <div class="tpl-a__sub tpl-a__sub--err" v-if="sub">{{ sub }}</div>
    </template>

    <!-- 更新日志：吸底占满剩余高度，条数不定也不会撑变形 -->
    <div class="tpl-a__log" v-if="changelog.length">
      <div class="tpl-a__log-title">更新日志</div>
      <ul class="tpl-a__log-list">
        <li v-for="(line, i) in changelog" :key="i">{{ line }}</li>
      </ul>
    </div>

    <div class="tpl-a__actions" v-if="showActions">
      <button class="tpl-a__btn tpl-a__btn--primary" :disabled="retrying" @click="$emit('retry')">
        {{ retrying ? '正在重试...' : '重试' }}
      </button>
      <button class="tpl-a__btn" @click="$emit('close')">关闭</button>
    </div>
    <div class="tpl-a__actions" v-else-if="stage === 'done'">
      <button class="tpl-a__btn tpl-a__btn--primary" @click="$emit('finish')">完成</button>
    </div>
  </div>
</template>

<script setup>
import { computed } from 'vue';
import { usePhase, STAGE } from './usePhase.js';

const props = defineProps({
  /// 阶段载荷（App.vue 统一维护，见 src/App.vue）
  phase: { type: Object, default: () => ({}) },
  /// 重试请求是否已被受理（错误态按钮禁用态）
  retrying: { type: Boolean, default: false },
});

defineEmits(['retry', 'close', 'finish']);

const { stage, message, sub, badge, percentValue, sizeText, speedText, etaText, changelog } =
  usePhase(() => props.phase);

const showActions = computed(() => stage.value === STAGE.ERROR);
</script>

<style scoped>
.tpl-a {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 16px;
  padding: 26px 28px 22px;
  box-sizing: border-box;
}

.tpl-a__head {
  display: flex;
  align-items: center;
  gap: 10px;
}

.tpl-a__head--split {
  justify-content: space-between;
}

.tpl-a__title {
  font-size: 16px;
  font-weight: 600;
  color: #e8eaf0;
}

.tpl-a__title--err {
  color: #ff5b66;
}

.tpl-a__sub {
  font-size: 12px;
  color: rgba(232, 234, 240, 0.55);
  word-break: break-all;
}

.tpl-a__sub--err {
  color: rgba(255, 91, 102, 0.75);
}

.tpl-a__pill {
  font-size: 11px;
  color: #4a9eff;
  background: rgba(74, 158, 255, 0.14);
  border: 1px solid rgba(74, 158, 255, 0.35);
  border-radius: 10px;
  padding: 3px 10px;
  flex-shrink: 0;
}

.tpl-a__metrics {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
}

.tpl-a__pct {
  font-size: 26px;
  font-weight: 600;
  color: #4a9eff;
  font-variant-numeric: tabular-nums;
}

.tpl-a__pct-u {
  font-size: 14px;
}

.tpl-a__speed {
  font-size: 12px;
  color: rgba(232, 234, 240, 0.6);
  font-variant-numeric: tabular-nums;
}

.tpl-a__track {
  width: 100%;
  height: 8px;
  border-radius: 4px;
  background: rgba(255, 255, 255, 0.1);
  overflow: hidden;
  flex-shrink: 0;
}

.tpl-a__fill {
  height: 100%;
  border-radius: 4px;
  background: linear-gradient(90deg, #4a9eff 0%, #35c26e 100%);
  transition: width 0.3s ease;
}

.tpl-a__meta {
  display: flex;
  justify-content: space-between;
  font-size: 12px;
  color: rgba(232, 234, 240, 0.55);
  font-variant-numeric: tabular-nums;
}

.tpl-a__dot {
  width: 14px;
  height: 14px;
  border-radius: 50%;
  flex-shrink: 0;
}

.tpl-a__dot--ok {
  background: #35c26e;
  box-shadow: 0 0 10px rgba(53, 194, 110, 0.5);
}

.tpl-a__dot--info {
  background: #4a9eff;
  box-shadow: 0 0 10px rgba(74, 158, 255, 0.5);
}

.tpl-a__dot--err {
  background: #e81123;
  box-shadow: 0 0 10px rgba(232, 17, 35, 0.5);
}

.tpl-a__spin {
  width: 18px;
  height: 18px;
  border: 2.5px solid rgba(255, 255, 255, 0.15);
  border-top-color: #4a9eff;
  border-radius: 50%;
  flex-shrink: 0;
  animation: tpl-a-spin 0.9s linear infinite;
}

.tpl-a__spin--green {
  border-top-color: #35c26e;
}

@keyframes tpl-a-spin {
  to { transform: rotate(360deg); }
}

.tpl-a__log {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  background: rgba(255, 255, 255, 0.04);
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 8px;
  padding: 12px 14px;
}

.tpl-a__log-title {
  font-size: 12px;
  color: rgba(232, 234, 240, 0.5);
  margin-bottom: 8px;
  flex-shrink: 0;
}

.tpl-a__log-list {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  margin: 0;
  padding: 0;
  list-style: none;
  font-size: 12.5px;
  line-height: 1.9;
  color: rgba(232, 234, 240, 0.85);
}

.tpl-a__log-list li {
  position: relative;
  padding-left: 13px;
  word-break: break-word;
}

.tpl-a__log-list li::before {
  content: '';
  position: absolute;
  left: 2px;
  top: 10px;
  width: 4px;
  height: 4px;
  border-radius: 50%;
  background: #4a9eff;
}

.tpl-a__log-list::-webkit-scrollbar { width: 6px; }
.tpl-a__log-list::-webkit-scrollbar-thumb {
  border-radius: 3px;
  background: rgba(255, 255, 255, 0.18);
}
.tpl-a__log-list::-webkit-scrollbar-track { background: transparent; }

.tpl-a__actions {
  display: flex;
  gap: 10px;
  margin-top: 2px;
  flex-shrink: 0;
}

.tpl-a__btn {
  padding: 7px 26px;
  border: 1px solid rgba(255, 255, 255, 0.2);
  border-radius: 6px;
  background: transparent;
  color: #e8eaf0;
  font-size: 13px;
  cursor: pointer;
  transition: background 0.2s, border-color 0.2s;
}

.tpl-a__btn:hover {
  background: rgba(255, 255, 255, 0.1);
  border-color: rgba(255, 255, 255, 0.35);
}

.tpl-a__btn--primary {
  border-color: #4a9eff;
  background: #4a9eff;
  color: #fff;
}

.tpl-a__btn--primary:hover:not(:disabled) {
  background: #357ac9;
  border-color: #357ac9;
}

.tpl-a__btn--primary:disabled {
  opacity: 0.6;
  cursor: default;
}
</style>
