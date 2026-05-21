<script setup lang="ts">
import type { BoardNote } from '../types';

const props = defineProps<{
  note: BoardNote;
}>();

const emit = defineEmits<{
  close: [];
}>();
</script>

<template>
  <section class="detail-panel visible" aria-live="polite" aria-label="sticky note details">
    <div class="detail-top">
      <div>
        <div class="detail-kicker">{{ props.note.kicker }}</div>
        <div class="detail-title">{{ props.note.title }}</div>
      </div>
      <button class="detail-close" type="button" aria-label="close detail panel" @click="emit('close')">×</button>
    </div>
    <div class="detail-desc">{{ props.note.description }}</div>
    <div class="detail-badges">
      <span class="detail-badge" :class="`status-${props.note.status}`">{{ props.note.statusLabel }}</span>
      <span v-if="props.note.agent" class="detail-badge agent">{{ props.note.agent }}</span>
    </div>
    <ul class="detail-tasks">
      <li v-for="(task, index) in props.note.tasks" :key="task" :class="{ done: index === 0 }">{{ task }}</li>
    </ul>
  </section>
</template>
