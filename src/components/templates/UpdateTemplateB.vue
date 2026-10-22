<template>
  <!-- 方案 B：环形进度 · 保留居中节奏，日志卡片固定高度贴底 -->
  <div class="tpl-b">
    <!-- 检测 / 合并中 -->
    <template v-if="stage === 'checking'">
      <div class="tpl-b__ring tpl-b__ring--idle">
        <div class="tpl-b__ring-in">
          <div class="tpl-b__spin"></div>
        </div>
      </div>
      <div class="tpl-b__title">{{ message || '正在检查更新...' }}</div>
      <div class="tpl-b__sub" v-if="sub">{{ sub }}</div>
    </template>

    <!-- 下载中 -->
    <template v-else-if="stage === 'downloading'">
      <div class="tpl-b__ring" :style="ringStyle">
        <div class="tpl-b__ring-in">
          <div class="tpl-b__pct">{{ percentValue.toFixed(0) }}<span class="tpl-b__pct-u">%</span></div>
          <div class="tpl-b__rate">{{ speedText }}</div>
        </div>
      </div>
      <div class="tpl-b__title">{{ message || '正在下载更新包...' }}</div>
      <div class="tpl-b__row">
        <span class="tpl-b__sub">{{ sizeText }}</span>
        <span class="tpl-b__pill" v-if="badge">{{ badge }}</span>
      </div>
      <div class="tpl-b__eta" v-if="etaText">{{ etaText }}</div>
    </template>

    <!-- 安装（解压）中 -->
    <template v-else-if="stage === 'installing'">
      <div class="tpl-b__ring" :style="ringStyle">
        <div class="tpl-b__ring-in">
          <div class="tpl-b__pct tpl-b__pct--green">{{ percentValue.toFixed(0) }}<span class="tpl-b__pct-u">%</span></div>
          <div class="tpl-b__rate">解压中</div>
        </div>
      </div>
      <div class="tpl-b__title">{{ message || '正在安装更新...' }}</div>
      <div class="tpl-b__sub" v-if="sub">{{ sub }}</div>
    </template>

    <!-- 已是最新 / 发现新版本 / 完成 / 错误：环换成状态圆点 -->
    <template v-else>
      <div
        class="tpl-b__dot"
        :class="{
          'tpl-b__dot--ok': stage === 'up-to-date' || stage === 'done',
          'tpl-b__dot--info': stage === 'available',
          'tpl-b__dot--err': stage === 'error',
        }"
      ></div>
      <div class="tpl-b__title" :class="{ 'tpl-b__title--err': stage === 'error' }">
        {{ message || (stage === 'up-to-date' ? '已是最新版本' : '更新完成') }}
      </div>
      <div class="tpl-b__sub" :class="{ 'tpl-b__sub--err': stage === 'error' }" v-if="sub">{{ sub }}</div>
    </template>

    <div class="tpl-b__log" v-if="changelog.length">
      <div class="tpl-b__log-title">更新日志</div>
      <ul class="tpl-b__log-list">
        <li v-for="(line, i) in changelog" :key="i">{{ line }}</li>
      </ul>
    </div>

    <div class="tpl-b__actions" v-if="stage === 'error'">
      <button class="tpl-b__btn tpl-b__btn--primary" :disabled="retrying" @click="$emit('retry')">
        {{ retrying ? '正在重试...' : '重试' }}
      </button>
      <button class="tpl-b__btn" @click="$emit('close')">关闭</button>
    </div>
    <div class="tpl-b__actions" v-else-if="stage === 'done'">
      <button class="tpl-b__btn tpl-b__btn--primary" @click="$emit('finish')">完成</button>
    </div>
  </div>
</template>

<script setup>
import { computed } from 'vue';
import { usePhase, STAGE } from './usePhase.js';

const props = defineProps({
  phase: { type: Object, default: () => ({}) },
  retrying: { type: Boolean, default: false },
});

defineEmits(['retry', 'close', 'finish']);

const { stage, message, sub, badge, percentValue, sizeText, speedText, etaText, changelog } =
  usePhase(() => props.phase);

/// 环形进度：conic-gradient 实现，无需额外 SVG
const ringStyle = computed(() => ({
  background: `conic-gradient(#4a9eff 0deg ${(percentValue.value * 3.6).toFixed(2)}deg, rgba(255,255,255,0.1) ${(percentValue.value * 3.6).toFixed(2)}deg 360deg)`,
}));

</script>

<style scoped>
.tpl-b {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 14px;
  padding: 14px 28px 20px;
  box-sizing: border-box;
}

.tpl-b__ring {
  width: 96px;
  height: 96px;
  border-radius: 50%;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: center;
}

.tpl-b__ring--idle {
  background: rgba(255, 255, 255, 0.1);
}

.tpl-b__ring-in {
  width: 78px;
  height: 78px;
  border-radius: 50%;
  background: #1a2029;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
}

.tpl-b__pct {
  font-size: 22px;
  font-weight: 600;
  color: #e8eaf0;
  font-variant-numeric: tabular-nums;
}

.tpl-b__pct--green {
  color: #35c26e;
}

.tpl-b__pct-u {
  font-size: 12px;
  color: rgba(232, 234, 240, 0.6);
}

.tpl-b__rate {
  font-size: 11px;
  color: rgba(232, 234, 240, 0.5);
  margin-top: 2px;
  font-variant-numeric: tabular-nums;
}

.tpl-b__spin {
  width: 26px;
  height: 26px;
  border: 3px solid rgba(255, 255, 255, 0.15);
  border-top-color: #4a9eff;
  border-radius: 50%;
  animation: tpl-b-spin 0.9s linear infinite;
}

@keyframes tpl-b-spin {
  to { transform: rotate(360deg); }
}

.tpl-b__title {
  font-size: 15px;
  font-weight: 600;
  color: #e8eaf0;
}

.tpl-b__title--err {
  color: #ff5b66;
}

.tpl-b__sub {
  font-size: 12px;
  color: rgba(232, 234, 240, 0.55);
  font-variant-numeric: tabular-nums;
  word-break: break-all;
}

.tpl-b__sub--err {
  color: rgba(255, 91, 102, 0.75);
}

.tpl-b__eta {
  font-size: 11px;
  color: rgba(232, 234, 240, 0.45);
}

.tpl-b__row {
  display: flex;
  align-items: center;
  gap: 10px;
}

.tpl-b__pill {
  font-size: 11px;
  color: #4a9eff;
  background: rgba(74, 158, 255, 0.14);
  border: 1px solid rgba(74, 158, 255, 0.35);
  border-radius: 10px;
  padding: 2px 9px;
}

.tpl-b__dot {
  width: 16px;
  height: 16px;
  border-radius: 50%;
  flex-shrink: 0;
}

.tpl-b__dot--ok {
  background: #35c26e;
  box-shadow: 0 0 10px rgba(53, 194, 110, 0.5);
}

.tpl-b__dot--info {
  background: #4a9eff;
  box-shadow: 0 0 10px rgba(74, 158, 255, 0.5);
}

.tpl-b__dot--err {
  background: #e81123;
  box-shadow: 0 0 10px rgba(232, 17, 35, 0.5);
}

.tpl-b__log {
  width: 100%;
  flex-shrink: 0;
  background: rgba(255, 255, 255, 0.04);
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 8px;
  padding: 11px 13px;
  box-sizing: border-box;
}

.tpl-b__log-title {
  font-size: 12px;
  color: rgba(232, 234, 240, 0.5);
  margin-bottom: 6px;
}

.tpl-b__log-list {
  max-height: 96px;
  overflow-y: auto;
  margin: 0;
  padding: 0;
  list-style: none;
  font-size: 12.5px;
  line-height: 1.85;
  color: rgba(232, 234, 240, 0.85);
}

.tpl-b__log-list li {
  position: relative;
  padding-left: 13px;
  word-break: break-word;
}

.tpl-b__log-list li::before {
  content: '';
  position: absolute;
  left: 2px;
  top: 9px;
  width: 4px;
  height: 4px;
  border-radius: 50%;
  background: #4a9eff;
}

.tpl-b__actions {
  display: flex;
  gap: 10px;
  flex-shrink: 0;
}

.tpl-b__btn {
  padding: 7px 26px;
  border: 1px solid rgba(255, 255, 255, 0.2);
  border-radius: 6px;
  background: transparent;
  color: #e8eaf0;
  font-size: 13px;
  cursor: pointer;
  transition: background 0.2s, border-color 0.2s;
}

.tpl-b__btn:hover {
  background: rgba(255, 255, 255, 0.1);
}

.tpl-b__btn--primary {
  border-color: #4a9eff;
  background: #4a9eff;
  color: #fff;
}

.tpl-b__btn--primary:disabled {
  opacity: 0.6;
  cursor: default;
}
</style>
