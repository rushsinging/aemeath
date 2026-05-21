<script setup lang="ts">
import type { BoardNote } from '../types';

const props = defineProps<{
  note: BoardNote;
  active: boolean;
}>();

const emit = defineEmits<{
  select: [noteId: string];
}>();
</script>

<template>
  <button
    class="note"
    :class="[props.note.tone, { active: props.active }]"
    type="button"
    :aria-pressed="props.active"
    @click="emit('select', props.note.id)"
    @focus="emit('select', props.note.id)"
  >
    <span class="note-title">{{ props.note.title }}</span>
    <span class="note-meta">
      <span class="note-status" :class="props.note.status">{{ props.note.statusLabel }}</span>
      <span v-if="props.note.agentLabel" class="note-agent">{{ props.note.agentLabel }}</span>
    </span>
  </button>
</template>
