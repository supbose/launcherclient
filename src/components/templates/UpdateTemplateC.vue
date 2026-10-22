<template>
  <!-- 方案 C：顶部步骤条（下载 → 校验 → 解压 → 完成）+ 顶对齐内容 + 日志卡片吸底 -->
  <div class="tpl-c">
    <!-- 常驻步骤条：让用户随时知道整体进度走到第几步 -->
    <div class="tpl-c__steps">
      <template v-for="(label, i) in stepLabels" :key="label">
        <div class="tpl-c__node">
          <div
            class="tpl-c__node-dot"
            :class="{
              'tpl-c__node-dot--done': i < step,
              'tpl-c__node-dot--on': i === step,
              'tpl-c__node-dot--err': i === step && stage === 'error',
            }"
          ></div>
          <div
            class="tpl-c__node-label"
            :class="{ 'tpl-c__node-label--on': i === step }"
          >{{ label }}</div>
        </div>
        <div
          v-if="i < stepLabels.length - 1"
          class="tpl-c__line"
          :class="{ 'tpl-c__line--done': i < step }"
        ></div>
      </template>
    </div>

    <!-- 检测 / 合并中 -->
    <template v-if="stage === 'checking'">
      <div class="tpl-c__head">
        <div class="tpl-c__title">{{ message || '正在检查更新...' }}</div>
        <div class="tpl-c__pill" v-if="badge">{{ badge }}</div>
      </div>
      <div class="tpl-c__row"><div class="tpl-c__spin"></div><div class="tpl-c__sub">{{ sub }}</div></div>
      <div class="tpl-c__track" v-if="sub && percentValue > 0">
        <div class="tpl-c__fill" :style="{ width: percentValue + '%' }"></div>
      </div>
    </template>

    <!-- 下载中 -->
    <template v-else-if="stage === 'downloading'">
      <div class="tpl-c__head">
        <div class="tpl-c__title">{{ message || '正在下载更新包...' }}</div>
        <div class="tpl-c__pill" v-if="badge">{{ badge }}</div>
      </div>
      <div class="tpl-c__metrics">
        <div class="tpl-c__pct">{{ percentValue.toFixed(1) }}<span class="tpl-c__pct-u">%</span></div>
        <div class="tpl-c__speed">{{ speedText }}</div>
      </div>
      <div class="tpl-c__track">
        <div class="tpl-c__fill" :style="{ width: percentValue + '%' }"></div>
      </div>
      <div class="tpl-c__meta"><span>{{ sizeText }}</span><span>{{ etaText }}</span></div>
    </template>

    <!-- 安装（解压）中 -->
    <template v-else-if="stage === 'installing'">
      <div class="tpl-c__head">
        <div class="tpl-c__title">{{ message || '正在安装更新...' }}</div>
      </div>
      <div class="tpl-c__row"><div class="tpl-c__spin tpl-c__spin--green"></div><div class="tpl-c__sub">{{ sub }}</div></div>
      <div class="tpl-c__track">
        <div class="tpl-c__fill" :style="{ width: percentValue + '%' }"></div>
      </div>
      <div class="tpl-c__meta"><span>{{ percentValue.toFixed(1) }}%</span><span>请勿关闭窗口</span></div>
    </template>

    <!-- 结束态（已是最新 / 发现新版本 / 完成 / 错误） -->
    <template v-else>
      <div class="tpl-c__head">
        <div
          class="tpl-c__dot"
          :class="{
            'tpl-c__dot--ok': stage === 'up-to-date' || stage === 'done',
            'tpl-c__dot--info': stage === 'available',
            'tpl-c__dot--err': stage === 'error',
          }"
        ></div>
        <div class="tpl-c__title" :class="{ 'tpl-c__title--err': stage === 'error' }">
          {{ message || (stage === 'up-to-date' ? '已是最新版本' : '更新完成') }}
        </div>
      </div>
      <div class="tpl-c__sub" :class="{ 'tpl-c__sub--err': stage === 'error' }" v-if="sub">{{ sub }}</div>
    </template>

    <div class="tpl-c__log" v-if="changelog.length">
      <div class="tpl-c__log-title">更新日志</div>
      <ul class="tpl-c__log-list">
        <li v-for="(line, i) in changelog" :key="i">{{ line }}</li>
      </ul>
    </div>

    <div class="tpl-c__actions" v-if="stage === 'error'">
      <button class="tpl-c__btn tpl-c__btn--primary" :disabled="retrying" @click="$emit('retry')">
        {{ retrying ? '正在重试...' : '重试' }}
      </button>
      <button class="tpl-c__btn" @click="$emit('close')">关闭</button>
    </div>
    <div class="tpl-c__actions" v-else-if="stage === 'done'">
      <button class="tpl-c__btn tpl-c__btn--primary" @click="$emit('finish')">完成</button>
    </div>
  </div>
</template>

<script setup>
import { usePhase, STEP_LABELS } from './usePhase.js';

const props = defineProps({
  phase: { type: Object, default: () => ({}) },
  retrying: { type: Boolean, default: false },
});

defineEmits(['retry', 'close', 'finish']);

const { stage, message, sub, badge, percentValue, sizeText, speedText, etaText, changelog, step } =
  usePhase(() => props.phase);

const stepLabels = STEP_LABELS;
</script>

<style scoped>
.tpl-c {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 14px;
  padding: 16px 28px 22px;
  box-sizing: border-box;
}

.tpl-c__steps {
  display: flex;
  align-items: flex-start;
  flex-shrink: 0;
}

.tpl-c__node {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 5px;
}

.tpl-c__node-dot {
  width: 16px;
  height: 16px;
  border-radius: 50%;
  background: rgba(255, 255, 255, 0.12);
  border: 1px solid rgba(255, 255, 255, 0.18);
  box-sizing: border-box;
  transition: background 0.2s, box-shadow 0.2s;
}

.tpl-c__node-dot--done {
  background: #35c26e;
  border: none;
}

.tpl-c__node-dot--on {
  background: #4a9eff;
  border: none;
  box-shadow: 0 0 0 4px rgba(74, 158, 255, 0.18);
}

.tpl-c__node-dot--err {
  background: #e81123;
  box-shadow: 0 0 0 4px rgba(232, 17, 35, 0.18);
}

.tpl-c__node-label {
  font-size: 11px;
  color: rgba(232, 234, 240, 0.45);
}

.tpl-c__node-label--on {
  color: #4a9eff;
}

.tpl-c__line {
  height: 2px;
  background: rgba(255, 255, 255, 0.14);
  margin-top: 7px;
  flex: 1;
}

.tpl-c__line--done {
  background: rgba(53, 194, 110, 0.5);
}

.tpl-c__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
}

.tpl-c__title {
  font-size: 16px;
  font-weight: 600;
  color: #e8eaf0;
}

.tpl-c__title--err {
  color: #ff5b66;
}

.tpl-c__pill {
  font-size: 11px;
  color: #4a9eff;
  background: rgba(74, 158, 255, 0.14);
  border: 1px solid rgba(74, 158, 255, 0.35);
  border-radius: 10px;
  padding: 3px 10px;
  flex-shrink: 0;
}

.tpl-c__row {
  display: flex;
  align-items: center;
  gap: 10px;
}

.tpl-c__sub {
  font-size: 12.5px;
  color: rgba(232, 234, 240, 0.65);
  word-break: break-all;
}

.tpl-c__sub--err {
  color: rgba(255, 91, 102, 0.75);
}

.tpl-c__spin {
  width: 18px;
  height: 18px;
  border: 2.5px solid rgba(255, 255, 255, 0.15);
  border-top-color: #4a9eff;
  border-radius: 50%;
  flex-shrink: 0;
  animation: tpl-c-spin 0.9s linear infinite;
}

.tpl-c__spin--green {
  border-top-color: #35c26e;
}

@keyframes tpl-c-spin {
  to { transform: rotate(360deg); }
}

.tpl-c__metrics {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
}

.tpl-c__pct {
  font-size: 26px;
  font-weight: 600;
  color: #4a9eff;
  font-variant-numeric: tabular-nums;
}

.tpl-c__pct-u {
  font-size: 14px;
}

.tpl-c__speed {
  font-size: 12px;
  color: rgba(232, 234, 240, 0.6);
  font-variant-numeric: tabular-nums;
}

.tpl-c__track {
  width: 100%;
  height: 8px;
  border-radius: 4px;
  background: rgba(255, 255, 255, 0.1);
  overflow: hidden;
  flex-shrink: 0;
}

.tpl-c__fill {
  height: 100%;
  border-radius: 4px;
  background: linear-gradient(90deg, #4a9eff 0%, #35c26e 100%);
  transition: width 0.3s ease;
}

.tpl-c__meta {
  display: flex;
  justify-content: space-between;
  font-size: 12px;
  color: rgba(232, 234, 240, 0.55);
  font-variant-numeric: tabular-nums;
}

.tpl-c__dot {
  width: 14px;
  height: 14px;
  border-radius: 50%;
  flex-shrink: 0;
}

.tpl-c__dot--ok {
  background: #35c26e;
  box-shadow: 0 0 10px rgba(53, 194, 110, 0.5);
}

.tpl-c__dot--info {
  background: #4a9eff;
  box-shadow: 0 0 10px rgba(74, 158, 255, 0.5);
}

.tpl-c__dot--err {
  background: #e81123;
  box-shadow: 0 0 10px rgba(232, 17, 35, 0.5);
}

.tpl-c__log {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  background: rgba(255, 255, 255, 0.04);
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 8px;
  padding: 12px 14px;
}

.tpl-c__log-title {
  font-size: 12px;
  color: rgba(232, 234, 240, 0.5);
  margin-bottom: 8px;
  flex-shrink: 0;
}

.tpl-c__log-list {
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

.tpl-c__log-list li {
  position: relative;
  padding-left: 13px;
  word-break: break-word;
}

.tpl-c__log-list li::before {
  content: '';
  position: absolute;
  left: 2px;
  top: 10px;
  width: 4px;
  height: 4px;
  border-radius: 50%;
  background: #4a9eff;
}

.tpl-c__actions {
  display: flex;
  gap: 10px;
  flex-shrink: 0;
}

.tpl-c__btn {
  padding: 7px 26px;
  border: 1px solid rgba(255, 255, 255, 0.2);
  border-radius: 6px;
  background: transparent;
  color: #e8eaf0;
  font-size: 13px;
  cursor: pointer;
  transition: background 0.2s, border-color 0.2s;
}

.tpl-c__btn:hover {
  background: rgba(255, 255, 255, 0.1);
}

.tpl-c__btn--primary {
  border-color: #4a9eff;
  background: #4a9eff;
  color: #fff;
}

.tpl-c__btn--primary:disabled {
  opacity: 0.6;
  cursor: default;
}
</style>
