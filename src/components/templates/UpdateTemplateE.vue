<template>
  <!-- 方案 E：日志为主 · 进度压成顶部状态条，更新日志占主体 -->
  <div class="tpl-e">
    <!-- 顶部紧凑状态条：指示灯 + 文案 + 百分比 -->
    <div class="tpl-e__bar">
      <div
        class="tpl-e__led"
        :class="{
          'tpl-e__led--ok': stage === 'up-to-date' || stage === 'done',
          'tpl-e__led--err': stage === 'error',
        }"
      ></div>
      <div class="tpl-e__title" :class="{ 'tpl-e__title--err': stage === 'error' }">
        {{ message || statusFallback }}
      </div>
      <div class="tpl-e__pct" v-if="showPercent">{{ percentValue.toFixed(1) }}%</div>
    </div>

    <!-- 进度条（下载 / 安装 / 合并） -->
    <div class="tpl-e__track" v-if="showProgress">
      <div class="tpl-e__fill" :style="{ width: percentValue + '%' }"></div>
    </div>

    <!-- 主体：更新日志 -->
    <div class="tpl-e__main">
      <div class="tpl-e__log-head">
        <div class="tpl-e__log-title">更新日志</div>
        <div class="tpl-e__log-count" v-if="changelog.length">共 {{ changelog.length }} 项</div>
      </div>
      <div class="tpl-e__log-list" v-if="changelog.length">
        <div class="tpl-e__log-item" v-for="(line, i) in changelog" :key="i">{{ line }}</div>
      </div>
      <div class="tpl-e__log-empty" v-else>本次更新未提供更新日志</div>
    </div>

    <!-- 副信息 / 底部明细 -->
    <div class="tpl-e__sub" v-if="sub && stage !== 'downloading'">{{ sub }}</div>
    <div class="tpl-e__foot">
      <span>{{ sizeText || (stage === 'error' ? '更新未完成' : '') }}</span>
      <span>{{ speedText || etaText }}</span>
    </div>

    <div class="tpl-e__actions" v-if="stage === 'error'">
      <button class="tpl-e__btn tpl-e__btn--primary" :disabled="retrying" @click="$emit('retry')">
        {{ retrying ? '正在重试...' : '重试' }}
      </button>
      <button class="tpl-e__btn" @click="$emit('close')">关闭</button>
    </div>
    <div class="tpl-e__actions" v-else-if="stage === 'done'">
      <button class="tpl-e__btn tpl-e__btn--primary" @click="$emit('finish')">完成</button>
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

const { stage, message, sub, percentValue, sizeText, speedText, etaText, changelog, showProgress } =
  usePhase(() => props.phase);

/// 结束态文案兜底（后端未带 message 时）
const statusFallback = computed(() => {
  switch (stage.value) {
    case STAGE.UP_TO_DATE:
      return '已是最新版本';
    case STAGE.AVAILABLE:
      return '发现新版本';
    case STAGE.CHECKING:
      return '正在检查更新...';
    case STAGE.DOWNLOADING:
      return '正在下载更新包...';
    case STAGE.INSTALLING:
      return '正在安装更新...';
    default:
      return '更新完成';
  }
});

/// 百分比只在有实际进度时显示（结束态不显示 0%）
const showPercent = computed(
  () =>
    stage.value === STAGE.DOWNLOADING ||
    stage.value === STAGE.INSTALLING ||
    (stage.value === STAGE.CHECKING && percentValue.value > 0)
);
</script>

<style scoped>
.tpl-e {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 14px;
  padding: 20px 26px 18px;
  box-sizing: border-box;
}

.tpl-e__bar {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-shrink: 0;
}

.tpl-e__led {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: #4a9eff;
  box-shadow: 0 0 8px rgba(74, 158, 255, 0.6);
  flex-shrink: 0;
}

.tpl-e__led--ok {
  background: #35c26e;
  box-shadow: 0 0 8px rgba(53, 194, 110, 0.6);
}

.tpl-e__led--err {
  background: #e81123;
  box-shadow: 0 0 8px rgba(232, 17, 35, 0.6);
}

.tpl-e__title {
  font-size: 14px;
  font-weight: 600;
  color: #e8eaf0;
}

.tpl-e__title--err {
  color: #ff5b66;
}

.tpl-e__pct {
  margin-left: auto;
  font-size: 14px;
  font-weight: 600;
  color: #4a9eff;
  font-variant-numeric: tabular-nums;
}

.tpl-e__track {
  width: 100%;
  height: 6px;
  border-radius: 3px;
  background: rgba(255, 255, 255, 0.1);
  overflow: hidden;
  flex-shrink: 0;
}

.tpl-e__fill {
  height: 100%;
  border-radius: 3px;
  background: linear-gradient(90deg, #4a9eff 0%, #35c26e 100%);
  transition: width 0.3s ease;
}

.tpl-e__main {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.tpl-e__log-head {
  display: flex;
  align-items: baseline;
  gap: 8px;
  margin-bottom: 10px;
  flex-shrink: 0;
}

.tpl-e__log-title {
  font-size: 14px;
  font-weight: 600;
  color: #e8eaf0;
}

.tpl-e__log-count {
  font-size: 11px;
  color: rgba(232, 234, 240, 0.4);
}

.tpl-e__log-list {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  font-size: 12.5px;
  line-height: 1.95;
  color: rgba(232, 234, 240, 0.85);
}

.tpl-e__log-item {
  position: relative;
  padding-left: 14px;
  word-break: break-word;
}

.tpl-e__log-item::before {
  content: '';
  position: absolute;
  left: 2px;
  top: 10px;
  width: 4px;
  height: 4px;
  border-radius: 50%;
  background: #4a9eff;
}

.tpl-e__log-empty {
  flex: 1;
  min-height: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 12.5px;
  color: rgba(232, 234, 240, 0.35);
}

.tpl-e__sub {
  font-size: 12px;
  color: rgba(232, 234, 240, 0.6);
  word-break: break-all;
  flex-shrink: 0;
}

.tpl-e__foot {
  display: flex;
  justify-content: space-between;
  font-size: 11.5px;
  color: rgba(232, 234, 240, 0.5);
  font-variant-numeric: tabular-nums;
  border-top: 0.5px solid rgba(255, 255, 255, 0.09);
  padding-top: 10px;
  flex-shrink: 0;
}

.tpl-e__actions {
  display: flex;
  gap: 10px;
  flex-shrink: 0;
}

.tpl-e__btn {
  padding: 7px 26px;
  border: 1px solid rgba(255, 255, 255, 0.2);
  border-radius: 6px;
  background: transparent;
  color: #e8eaf0;
  font-size: 13px;
  cursor: pointer;
  transition: background 0.2s, border-color 0.2s;
}

.tpl-e__btn:hover {
  background: rgba(255, 255, 255, 0.1);
}

.tpl-e__btn--primary {
  border-color: #4a9eff;
  background: #4a9eff;
  color: #fff;
}

.tpl-e__btn--primary:disabled {
  opacity: 0.6;
  cursor: default;
}
</style>
