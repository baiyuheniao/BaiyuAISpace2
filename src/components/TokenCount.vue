<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<script setup lang="ts">
import { computed } from "vue";
import { formatTokenCount } from "@/utils/tokenCount";

const props = withDefaults(defineProps<{
  count: number;
  label?: string;
  description?: string;
}>(), {
  label: "",
  description: "估算值，仅统计可见文本；不含图片、隐藏系统提示词和工具上下文",
});

const formattedCount = computed(() => formatTokenCount(props.count));
</script>

<template>
  <span
    class="token-count"
    :title="description"
  >
    <span
      v-if="label"
      class="token-label"
    >{{ label }}</span>
    <span class="token-value">≈ {{ formattedCount }} Tokens</span>
  </span>
</template>

<style scoped lang="scss">
.token-count {
  display: inline-flex;
  align-items: baseline;
  gap: 0.45rem;
  color: $ink-faint;
  font-family: $font-mono;
  font-size: 11px;
  line-height: 1.2;
  white-space: nowrap;
}

.token-label {
  font-family: $font-sans;
  font-size: 10px;
  font-weight: 500;
  letter-spacing: 0.1em;
  text-transform: uppercase;
}

.token-value {
  font-variant-numeric: tabular-nums;
}
</style>
