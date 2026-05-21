<script setup lang="ts">
import { computed } from 'vue';
import BoardDrawers from './components/BoardDrawers.vue';
import ChatDock from './components/ChatDock.vue';
import NoteDetailPanel from './components/NoteDetailPanel.vue';
import StickyNote from './components/StickyNote.vue';
import { useBoardStore } from './stores/board';

const board = useBoardStore();
const selectedNote = computed(() => board.selectedNote);
</script>

<template>
  <main class="app" aria-label="Aemeath fullscreen whiteboard workbench">
    <BoardDrawers :workspaces="board.workspaces" :agents="board.agents" />

    <div class="tools" aria-label="whiteboard tools">
      <span class="tool active">↖</span>
      <span class="tool">T</span>
      <span class="tool">□</span>
      <span class="tool">✎</span>
      <span class="tool">⟲</span>
    </div>

    <section class="board-stage" aria-label="board canvas">
      <div class="note-row requirement-row" aria-label="Requirements">
        <StickyNote
          v-for="note in board.requirementNotes"
          :key="note.id"
          :note="note"
          :active="note.id === board.selectedNoteId"
          @select="board.selectNote"
        />
      </div>

      <div class="note-row project-row" aria-label="Projects">
        <StickyNote
          v-for="note in board.projectNotes"
          :key="note.id"
          :note="note"
          :active="note.id === board.selectedNoteId"
          @select="board.selectNote"
        />
      </div>
    </section>

    <NoteDetailPanel v-if="selectedNote" :note="selectedNote" @close="board.clearSelection" />
    <ChatDock />
  </main>
</template>
