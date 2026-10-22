<template>
  <div class="pv">
    <div class="pv__panel">
      <h1 class="pv__h1">启动器界面模板预览</h1>
      <p class="pv__lead">
        五套模板共用同一份阶段数据，样式各自独立（scoped + 类前缀，互不覆盖）。切换选项对比。
      </p>

      <div class="pv__field">
        <label class="pv__label">界面模板（launcher_template）</label>
        <div class="pv__chips">
          <button
            v-for="key in templateKeys"
            :key="key"
            class="pv__chip"
            :class="{ 'pv__chip--on': template === key }"
            @click="template = key"
          >
            {{ key }} · {{ meta[key].name }}
          </button>
        </div>
        <p class="pv__hint">{{ meta[template].desc }}</p>
      </div>

      <div class="pv__field">
        <label class="pv__label">阶段</label>
        <div class="pv__chips">
          <button
            v-for="(preset, key) in presets"
            :key="key"
            class="pv__chip pv__chip--sm"
            :class="{ 'pv__chip--on': stageKey === key }"
            @click="stageKey = key"
          >
            {{ preset.label }}
          </button>
        </div>
      </div>

      <div class="pv__field">
        <label class="pv__label">按钮状态</label>
        <label class="pv__check">
          <input type="checkbox" v-model="retrying" />
          重试中（禁用重试按钮）
        </label>
        <p class="pv__hint">最近事件：{{ lastEvent }}</p>
      </div>
    </div>

    <div class="pv__stage">
      <div class="pv__win">
        <div class="pv__titlebar"><span>—</span><span>✕</span></div>
        <component
          :is="activeComponent"
          :phase="phase"
          :retrying="retrying"
          @retry="onEvent('retry')"
          @close="onEvent('close')"
          @finish="onEvent('finish')"
        />
      </div>
      <p class="pv__caption">
        窗口 440×420 · 当前模板 {{ template }} · 阶段 {{ presets[stageKey].label }}
      </p>
    </div>
  </div>
</template>

<script setup>
import { ref, computed } from 'vue';
import {
  TEMPLATE_KEYS,
  TEMPLATE_META,
  DEFAULT_TEMPLATE,
  templateComponent,
  resolveTemplateKey,
} from './components/templates/index.js';

const CHANGELOG = [
  '新增分卷下载支持，大包更新更稳定',
  '修复更新后启动旧版二进制的问题',
  '优化启动速度与安装速度',
  '修复了某些 bugs',
];

/// 阶段预设（字段与后端 PhasePayload 一一对应）
const STAGE_PRESETS = {
  checking: {
    label: '检测中',
    phase: { stage: 'checking', message: '正在检查更新...', sub: '', percent: 0, step: -1 },
  },
  downloading: {
    label: '下载中（分卷）',
    phase: {
      stage: 'downloading',
      message: '正在下载更新包...',
      badge: '分卷 1/28',
      percent: 3.4,
      downloadedBytes: 4.67 * 1024 * 1024,
      totalBytes: 138.39 * 1024 * 1024,
      speedBps: 112 * 1024,
      changelog: CHANGELOG,
      step: 0,
    },
  },
  verifying: {
    label: '校验中',
    phase: {
      stage: 'checking',
      message: '正在校验更新包...',
      sub: '校验 SHA256 完整性，确保更新包未被篡改',
      percent: 0,
      changelog: CHANGELOG,
      step: 1,
    },
  },
  installing: {
    label: '解压/安装中',
    phase: {
      stage: 'installing',
      message: '正在安装更新...',
      sub: '正在解压更新文件...',
      percent: 62,
      changelog: CHANGELOG,
      step: 2,
    },
  },
  available: {
    label: '发现新版本',
    phase: {
      stage: 'available',
      message: '发现新版本',
      sub: '2.1.2 → 2.1.3',
      changelog: CHANGELOG,
      step: 0,
    },
  },
  'up-to-date': {
    label: '已是最新',
    phase: { stage: 'up-to-date', message: '已是最新版本', sub: '2.1.3', step: 3 },
  },
  done: {
    label: '更新完成',
    phase: {
      stage: 'done',
      message: '更新完成',
      sub: '已更新到版本 2.1.3',
      changelog: CHANGELOG,
      step: 3,
    },
  },
  error: {
    label: '错误',
    phase: {
      stage: 'error',
      message: '更新包下载失败',
      sub: '全部 3 个 manifest 地址均拉取失败',
      step: 0,
    },
  },
};

const templateKeys = TEMPLATE_KEYS;
const meta = TEMPLATE_META;
const presets = STAGE_PRESETS;

const template = ref(DEFAULT_TEMPLATE);
const stageKey = ref('downloading');
const retrying = ref(false);
const lastEvent = ref('—');

const activeComponent = computed(() => templateComponent(template.value));
const phase = computed(() => STAGE_PRESETS[stageKey.value].phase);

const onEvent = (name) => {
  lastEvent.value = `${name}（模板 ${resolveTemplateKey(template.value)}）`;
};
</script>
